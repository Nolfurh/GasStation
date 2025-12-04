// @generated automatically by Diesel CLI.

diesel::table! {
    admin (id) {
        id -> Integer,
        login -> Text,
        password -> Text,
        salt -> Text,
        session_token -> Nullable<Text>,
    }
}

diesel::table! {
    bank (id) {
        id -> Integer,
        total -> BigInt,
    }
}

diesel::table! {
    customer (id) {
        id -> Integer,
        login -> Text,
        password -> Text,
        salt -> Text,
        balance -> BigInt,
        session_token -> Nullable<Text>,
    }
}

diesel::table! {
    fuel (id) {
        id -> Integer,
        name -> Text,
        price -> BigInt,
        fuel_type -> Nullable<Text>,
    }
}

diesel::table! {
    tank (id) {
        id -> Integer,
        fuelid -> Integer,
        stored -> Integer,
        capacity -> Integer,
    }
}

diesel::joinable!(tank -> fuel (fuelid));

diesel::allow_tables_to_appear_in_same_query!(admin, bank, customer, fuel, tank,);
