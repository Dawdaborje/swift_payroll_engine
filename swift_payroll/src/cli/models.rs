use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct RawEmployee {
    pub employee_id: String,
    pub full_name: String,
    pub base_salary: rust_decimal::Decimal,
    pub allowances: RawAllowance,
    pub deductions: Vec<RawDeduction>,
    pub pay_period: Option<String>,
    #[serde(default)]
    pub results: Option<RawExpectedResults>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawAllowance {
    pub transport: rust_decimal::Decimal,
    pub housing: rust_decimal::Decimal,
    pub meal: rust_decimal::Decimal,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawDeduction {
    pub id: String,
    pub label: String,
    pub engine: String,
    pub expression: Option<String>,
    pub amount: Option<rust_decimal::Decimal>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawExpectedResults {
    pub gross_salary: rust_decimal::Decimal,
    pub net_salary: rust_decimal::Decimal,
    pub deductions: Vec<RawExpectedDeduction>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RawExpectedDeduction {
    pub id: String,
    pub amount: rust_decimal::Decimal,
}
