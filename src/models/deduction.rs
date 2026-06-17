use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DeductionType {
    pub id: Uuid,
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PayrollDeduction {
    pub id: Uuid,
    pub line_id: Uuid,
    pub deduction_type_id: Uuid,
    pub amount_cents: i64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PayrollDeductionWithType {
    pub id: Uuid,
    pub line_id: Uuid,
    pub deduction_type_id: Uuid,
    pub code: String,
    pub name: String,
    pub amount_cents: i64,
    pub note: Option<String>,
}
