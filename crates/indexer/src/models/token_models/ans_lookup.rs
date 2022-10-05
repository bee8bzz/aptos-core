// Copyright (c) Aptos
// SPDX-License-Identifier: Apache-2.0

// This is required because a diesel macro makes clippy sad
#![allow(clippy::extra_unused_lifetimes)]
#![allow(clippy::unused_unit)]

use std::collections::HashMap;

use crate::{
    schema::current_ans_lookup,
    util::{bigdecimal_to_u64, parse_timestamp_secs},
};
use aptos_api_types::{deserialize_from_string, MoveType, Transaction as APITransaction};
use bigdecimal::BigDecimal;
use field_count::FieldCount;
use serde::{Deserialize, Serialize};

type Domain = String;
type Subdomain = String;
// PK of current_ans_lookup, i.e. domain and subdomain name
pub type CurrentAnsLookupPK = (Domain, Subdomain);

#[derive(Debug, Deserialize, FieldCount, Identifiable, Insertable, Queryable, Serialize)]
#[diesel(primary_key(domain, subdomain))]
#[diesel(table_name = current_ans_lookup)]
pub struct CurrentAnsLookup {
    pub domain: String,
    pub subdomain: String,
    pub registered_address: Option<String>,
    pub last_transaction_version: i64,
    pub expiration_timestamp: chrono::NaiveDateTime,
    pub inserted_at: chrono::NaiveDateTime,
}

pub enum ANSEvent {
    SetNameAddressEventV1(SetNameAddressEventV1),
    RegisterNameEventV1(RegisterNameEventV1),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SetNameAddressEventV1 {
    subdomain_name: OptionalString,
    domain_name: String,
    new_address: OptionalString,
    #[serde(deserialize_with = "deserialize_from_string")]
    expiration_time_secs: BigDecimal,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegisterNameEventV1 {
    subdomain_name: OptionalString,
    domain_name: String,
    #[serde(deserialize_with = "deserialize_from_string")]
    expiration_time_secs: BigDecimal,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OptionalString {
    vec: Vec<String>,
}

impl OptionalString {
    fn get_string(&self) -> Option<String> {
        if self.vec.is_empty() {
            None
        } else {
            Some(self.vec[0].clone())
        }
    }
}

impl CurrentAnsLookup {
    pub fn from_transaction(
        transaction: &APITransaction,
        ans_contract_address: Option<String>,
    ) -> HashMap<CurrentAnsLookupPK, Self> {
        let mut current_ans_lookups: HashMap<CurrentAnsLookupPK, Self> = HashMap::new();
        if let Some(addr) = ans_contract_address {
            if let APITransaction::UserTransaction(user_txn) = transaction {
                for event in &user_txn.events {
                    let (event_addr, event_type) = if let MoveType::Struct(inner) = &event.typ {
                        (
                            inner.address.to_string(),
                            format!("{}::{}", inner.module, inner.name),
                        )
                    } else {
                        continue;
                    };
                    if event_addr != addr {
                        continue;
                    }
                    let txn_version = user_txn.info.version.0 as i64;
                    let maybe_ans_event = match event_type.as_str() {
                        "domains::SetNameAddressEventV1" => {
                            serde_json::from_value(event.data.clone())
                                .map(|inner| Some(ANSEvent::SetNameAddressEventV1(inner)))
                        }
                        "domains::RegisterNameEventV1" => {
                            serde_json::from_value(event.data.clone())
                                .map(|inner| Some(ANSEvent::RegisterNameEventV1(inner)))
                        }
                        _ => Ok(None),
                    }
                    .unwrap_or_else(|e| {
                        panic!(
                            "version {} failed! failed to parse type {}, data {:?}. Error: {:?}",
                            txn_version, event_type, event.data, e
                        )
                    });
                    if let Some(ans_event) = maybe_ans_event {
                        let current_ans_lookup = match ans_event {
                            ANSEvent::SetNameAddressEventV1(inner) => {
                                let expiration_timestamp = parse_timestamp_secs(
                                    bigdecimal_to_u64(&inner.expiration_time_secs),
                                    txn_version,
                                );
                                Self {
                                    domain: inner.domain_name,
                                    subdomain: inner
                                        .subdomain_name
                                        .get_string()
                                        .unwrap_or_default(),
                                    registered_address: inner.new_address.get_string(),
                                    last_transaction_version: txn_version,
                                    expiration_timestamp,
                                    inserted_at: chrono::Utc::now().naive_utc(),
                                }
                            }
                            ANSEvent::RegisterNameEventV1(inner) => {
                                let expiration_timestamp = parse_timestamp_secs(
                                    bigdecimal_to_u64(&inner.expiration_time_secs),
                                    txn_version,
                                );
                                Self {
                                    domain: inner.domain_name,
                                    subdomain: inner
                                        .subdomain_name
                                        .get_string()
                                        .unwrap_or_default(),
                                    registered_address: None,
                                    last_transaction_version: txn_version,
                                    expiration_timestamp,
                                    inserted_at: chrono::Utc::now().naive_utc(),
                                }
                            }
                        };

                        current_ans_lookups.insert(
                            (
                                current_ans_lookup.domain.clone(),
                                current_ans_lookup.subdomain.clone(),
                            ),
                            current_ans_lookup,
                        );
                    }
                }
            }
        }
        current_ans_lookups
    }
}
