// Functions for business logic

pub fn calculate_fuel_cost(price_per_unit: i64, amount: i32) -> i64 {
    price_per_unit * amount as i64
}

pub fn calculate_tank_percentage(stored: i32, capacity: i32) -> f64 {
    if capacity == 0 {
        return 0.0;
    }
    (stored as f64 / capacity as f64) * 100.0
}

pub fn format_money(cents: i64) -> String {
    format!("{:.2} грн", cents as f64 / 100.0)
}

pub fn calculate_refill_cost(fuel_price: i64, amount: i32) -> i64 {
    let cost_per_unit = fuel_price / 2;
    cost_per_unit * amount as i64
}

pub fn has_sufficient_balance(balance: i64, cost: i64) -> bool {
    balance >= cost
}

pub fn calculate_total_stored(tanks: &[(i32, i32)]) -> i32 {
    tanks.iter().map(|(stored, _)| stored).sum()
}

pub fn has_sufficient_fuel(total_stored: i32, amount_needed: i32) -> bool {
    total_stored >= amount_needed
}
