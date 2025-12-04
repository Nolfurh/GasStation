use crate::schema::*;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Queryable, Selectable, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[diesel(table_name = customer)]
pub struct Customer {
    pub id: i32,
    pub login: String,
    pub balance: i64,
    pub session_token: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = customer)]
pub struct NewCustomer<'a> {
    pub login: &'a str,
    pub password: &'a str,
    pub salt: &'a str,
    pub balance: i64,
}

#[derive(Queryable, Selectable, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[diesel(table_name = admin)]
pub struct Admin {
    pub id: i32,
    pub login: String,
    pub session_token: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = admin)]
pub struct NewAdmin<'a> {
    pub login: &'a str,
    pub password: &'a str,
    pub salt: &'a str,
}

#[derive(Queryable, Selectable, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[diesel(table_name = fuel)]
pub struct Fuel {
    pub id: i32,
    pub name: String,
    pub price: i64,
    pub fuel_type: Option<String>, // Нове поле
}

#[derive(Queryable, Selectable, Debug, Clone)]
#[diesel(table_name = tank)]
pub struct Tank {
    pub id: i32,
    pub fuelid: i32,
    pub stored: i32,
    pub capacity: i32,
}

#[derive(Queryable, Selectable, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[diesel(table_name = bank)]
pub struct Bank {
    pub id: i32,
    pub total: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FuelWithTank {
    pub id: i32,
    pub name: String,
    pub price: i64,
    pub fuel_type: String,
    pub stored: i32,
    pub capacity: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FuelPriceStats {
    pub name: String,
    pub average: f64,
    pub min: f64,
    pub max: f64,
    pub count: usize,
}

use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct FuelCardProps {
    pub item: FuelWithTank,
    pub cart: Signal<std::collections::HashMap<i32, i32>>,
}
