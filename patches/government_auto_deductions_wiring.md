# Government auto-deductions — wiring for blocked files

New files already created (writable):

- `src/services/payroll/gov_deductions.rs` — calculator + unit tests
- `src/services/payroll/deductions_auto.rs` — apply/recalculate/preflight logic
- `migrations/025_government_auto_deductions.sql` — DB columns

Apply the edits below to blocked files, then run `cargo test`.

---

## 1. `src/models/settings.rs`

Add after `PayPeriodType` enum:

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GovernmentDeductionToggles {
    pub sss: bool,
    pub phic: bool,
    pub hdmf: bool,
    pub wht: bool,
}

impl GovernmentDeductionToggles {
    pub fn any_enabled(self) -> bool {
        self.sss || self.phic || self.hdmf || self.wht
    }

    pub fn is_enabled_for_code(self, code: &str) -> bool {
        match code {
            "SSS" => self.sss,
            "PHIC" => self.phic,
            "HDMF" => self.hdmf,
            "WHT" => self.wht,
            _ => false,
        }
    }
}
```

Add to `CompanySettings` struct:

```rust
    pub auto_deduct_sss: bool,
    pub auto_deduct_phic: bool,
    pub auto_deduct_hdmf: bool,
    pub auto_deduct_wht: bool,
```

Add impl block:

```rust
impl CompanySettings {
    pub fn government_deduction_toggles(&self) -> GovernmentDeductionToggles {
        GovernmentDeductionToggles {
            sss: self.auto_deduct_sss,
            phic: self.auto_deduct_phic,
            hdmf: self.auto_deduct_hdmf,
            wht: self.auto_deduct_wht,
        }
    }
}
```

## 2. `src/models/mod.rs`

```rust
pub use settings::{CompanySettings, GovernmentDeductionToggles, PayPeriodType};
```

## 3. `src/services/settings.rs`

Extend `SettingsUpdate`:

```rust
    pub auto_deduct_sss: bool,
    pub auto_deduct_phic: bool,
    pub auto_deduct_hdmf: bool,
    pub auto_deduct_wht: bool,
```

Update `get_settings` SELECT + `update_settings` UPDATE/RETURNING to include:

```
auto_deduct_sss, auto_deduct_phic, auto_deduct_hdmf, auto_deduct_wht
```

Bind `$13`–`$16` in update (adjust indices after journal fields).

## 4. `src/services/payroll/mod.rs`

```rust
pub mod deductions_auto;
pub mod gov_deductions;
```

Add exports:

```rust
pub use deductions_auto::{
    apply_automatic_deductions_for_run, preflight_finalize_run, recalculate_government_deductions_for_run,
    FinalizePreflight,
};
pub use gov_deductions::{
    is_government_deduction_code, period_government_deductions_cents, AUTO_NOTE,
};
```

In `deductions` exports, keep `apply_deduction_defaults_for_run` as alias or remove and use automatic only.

## 5. `src/services/payroll/deductions.rs`

Replace `apply_deduction_defaults_for_run` body with delegation:

```rust
pub async fn apply_deduction_defaults_for_run(pool: &PgPool, run_id: Uuid) -> AppResult<()> {
    let settings = crate::services::settings::get_settings(pool).await?;
    super::deductions_auto::apply_automatic_deductions_for_run(pool, run_id, &settings).await
}
```

## 6. `src/services/payroll/runs.rs`

Change import:

```rust
use super::deductions_auto::{apply_automatic_deductions_for_run, preflight_finalize_run};
```

In `create_draft_run`, replace:

```rust
apply_deduction_defaults_for_run(pool, run_id).await?;
```

with:

```rust
apply_automatic_deductions_for_run(pool, run_id, settings).await?;
```

In `finalize_run`, before `refresh_all_line_net_pay`:

```rust
    let settings = crate::services::settings::get_settings(pool).await?;
    let preflight = preflight_finalize_run(pool, run_id, &settings).await?;
    if preflight.has_blocking_issues() {
        return Err(AppError::bad_request(format!(
            "Cannot finalize: {} employee line(s) have deductions exceeding gross pay",
            preflight.over_gross_count
        )));
    }
```

Export `preflight_finalize_run` from runs if needed by handlers.

## 7. `src/handlers/admin/settings.rs`

Add to page context `settings`:

```rust
auto_deduct_sss => settings.auto_deduct_sss,
auto_deduct_phic => settings.auto_deduct_phic,
auto_deduct_hdmf => settings.auto_deduct_hdmf,
auto_deduct_wht => settings.auto_deduct_wht,
```

Add to `SettingsForm`:

```rust
    auto_deduct_sss: Option<String>,
    auto_deduct_phic: Option<String>,
    auto_deduct_hdmf: Option<String>,
    auto_deduct_wht: Option<String>,
```

Pass to `SettingsUpdate` in `save_settings`.

## 8. `src/handlers/admin/payroll.rs`

Import:

```rust
use crate::services::payroll::{
    preflight_finalize_run, recalculate_government_deductions_for_run,
};
```

In `payroll_run_page`, load preflight + toggles, add context:

```rust
gov_auto_enabled => settings.government_deduction_toggles().any_enabled(),
preflight_stale_gov => preflight.stale_government_count,
preflight_missing_gov => preflight.missing_government_count,
preflight_over_gross => preflight.over_gross_count,
preflight_disabled_auto => preflight.disabled_auto_remaining_count,
has_gov_preflight_warnings => preflight.has_warnings(),
```

Add handler:

```rust
pub async fn recalculate_government_deductions_action(...) -> AppResult<Redirect> {
    let settings = get_settings(&state.pool).await?;
    recalculate_government_deductions_for_run(&state.pool, run_id, &settings).await?;
    // log + redirect_with_flash success
}
```

## 9. `src/handlers/admin/mod.rs`

```rust
pub use payroll::{
    ...,
    recalculate_government_deductions_action,
};
```

## 10. `src/app.rs`

```rust
.route(
    "/admin/payroll/{run_id}/recalculate-government-deductions",
    post(admin::recalculate_government_deductions_action),
)
```

## 11. Templates

See `templates/admin/_snippets/government_deductions.html` for copy-paste blocks.

## 12. Delete

Remove `src/services/payroll/deductions_apply.rs` if present (already deleted).