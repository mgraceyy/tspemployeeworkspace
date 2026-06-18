use printpdf::*;
use sqlx::PgPool;
use uuid::Uuid;

use super::payslips::get_payslip_for_employee;
use crate::error::{AppError, AppResult};
use crate::services::compensation::format_salary_cents;
use crate::services::reports::period_label_for_range;
use crate::services::timezone::format_date;

pub async fn build_payslip_pdf(
    pool: &PgPool,
    line_id: Uuid,
    employee_id: Uuid,
) -> AppResult<Vec<u8>> {
    let detail = get_payslip_for_employee(pool, employee_id, line_id).await?;
    let period_label = period_label_for_range(detail.period_start, detail.period_end);

    let (doc, page1, layer1) = PdfDocument::new("Payslip", Mm(210.0), Mm(297.0), "Layer 1");
    let font = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| AppError::Internal(e.into()))?;
    let font_bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(|e| AppError::Internal(e.into()))?;
    let current_layer = doc.get_page(page1).get_layer(layer1);

    let mut y = 280.0f32;
    let left = Mm(20.0);
    let line_height = 6.0;

    fn write_line(
        layer: &PdfLayerReference,
        font: &IndirectFontRef,
        size: f32,
        x: Mm,
        y: &mut f32,
        text: &str,
        line_height: f32,
    ) {
        layer.use_text(text, size, x, Mm(*y), font);
        *y -= line_height;
    }

    write_line(
        &current_layer,
        &font_bold,
        16.0,
        left,
        &mut y,
        "Payslip",
        line_height + 2.0,
    );
    write_line(
        &current_layer,
        &font,
        11.0,
        left,
        &mut y,
        &format!("Period: {period_label}"),
        line_height,
    );
    write_line(
        &current_layer,
        &font,
        11.0,
        left,
        &mut y,
        &format!("Employee: {} ({})", detail.full_name, detail.employee_code),
        line_height,
    );
    if let Some(ref dept) = detail.department {
        if !dept.is_empty() {
            write_line(
                &current_layer,
                &font,
                11.0,
                left,
                &mut y,
                &format!("Department: {dept}"),
                line_height,
            );
        }
    }
    y -= 4.0;
    write_line(
        &current_layer,
        &font_bold,
        12.0,
        left,
        &mut y,
        "Earnings",
        line_height,
    );
    write_line(
        &current_layer,
        &font,
        10.0,
        left,
        &mut y,
        &format!(
            "Base pay: PHP {}",
            format_salary_cents(detail.base_pay_cents)
        ),
        line_height,
    );
    if detail.allowance_cents > 0 {
        write_line(
            &current_layer,
            &font,
            10.0,
            left,
            &mut y,
            &format!(
                "Allowances: PHP {}",
                format_salary_cents(detail.allowance_cents)
            ),
            line_height,
        );
    }
    if detail.ot_pay_cents > 0 {
        write_line(
            &current_layer,
            &font,
            10.0,
            left,
            &mut y,
            &format!("OT pay: PHP {}", format_salary_cents(detail.ot_pay_cents)),
            line_height,
        );
    }
    if detail.no_show_deduction_cents > 0 {
        write_line(
            &current_layer,
            &font,
            10.0,
            left,
            &mut y,
            &format!(
                "No-show deduction: -PHP {}",
                format_salary_cents(detail.no_show_deduction_cents)
            ),
            line_height,
        );
    }
    write_line(
        &current_layer,
        &font_bold,
        10.0,
        left,
        &mut y,
        &format!("Gross: PHP {}", format_salary_cents(detail.gross_pay_cents)),
        line_height,
    );
    y -= 4.0;
    write_line(
        &current_layer,
        &font_bold,
        12.0,
        left,
        &mut y,
        "Deductions",
        line_height,
    );
    if detail.deductions.is_empty() {
        write_line(
            &current_layer,
            &font,
            10.0,
            left,
            &mut y,
            "None",
            line_height,
        );
    } else {
        for d in &detail.deductions {
            write_line(
                &current_layer,
                &font,
                10.0,
                left,
                &mut y,
                &format!("{}: PHP {}", d.name, format_salary_cents(d.amount_cents)),
                line_height,
            );
        }
    }
    write_line(
        &current_layer,
        &font_bold,
        10.0,
        left,
        &mut y,
        &format!(
            "Total deductions: PHP {}",
            format_salary_cents(detail.total_deduction_cents)
        ),
        line_height,
    );
    y -= 4.0;
    write_line(
        &current_layer,
        &font_bold,
        14.0,
        left,
        &mut y,
        &format!("Net pay: PHP {}", format_salary_cents(detail.net_pay_cents)),
        line_height + 2.0,
    );
    write_line(
        &current_layer,
        &font,
        9.0,
        left,
        &mut y,
        &format!("Finalized: {}", format_date(detail.finalized_at.date())),
        line_height,
    );

    let mut buffer = Vec::new();
    doc.save(&mut std::io::BufWriter::new(&mut buffer))
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(buffer)
}
