//! Dinheiro sempre em centavos (i64), nunca em ponto flutuante.

/// Converte texto digitado ("12,50", "12.5", "1.234,56", "10") em centavos.
pub fn parse_cents(input: &str) -> Option<i64> {
    let s: String = input
        .trim()
        .trim_start_matches("R$")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if s.is_empty() {
        return None;
    }
    // O último separador (, ou .) é o decimal se tiver 1 ou 2 dígitos depois dele.
    let (int_part, frac_part) = match s.rfind([',', '.']) {
        Some(i) if (1..=2).contains(&(s.len() - i - 1)) => (&s[..i], &s[i + 1..]),
        _ => (s.as_str(), ""),
    };
    let int_digits: String = int_part.chars().filter(|c| *c != '.' && *c != ',').collect();
    if !int_digits.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let reais: i64 = if int_digits.is_empty() { 0 } else { int_digits.parse().ok()? };
    let mut frac = frac_part.to_string();
    while frac.len() < 2 {
        frac.push('0');
    }
    let cents: i64 = frac.parse().ok()?;
    reais.checked_mul(100)?.checked_add(cents)
}

/// Formata centavos como "R$ 1.234,56".
pub fn format_cents(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let (reais, c) = (abs / 100, abs % 100);
    let digits = reais.to_string();
    let mut grouped = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(ch);
    }
    format!("{sign}R$ {grouped},{c:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(parse_cents("12,50"), Some(1250));
        assert_eq!(parse_cents("12.5"), Some(1250));
        assert_eq!(parse_cents("1.234,56"), Some(123456));
        assert_eq!(parse_cents("10"), Some(1000));
        assert_eq!(parse_cents("R$ 7,05"), Some(705));
        assert_eq!(parse_cents("1.000"), Some(100000));
        assert_eq!(parse_cents("abc"), None);
        assert_eq!(parse_cents(""), None);
    }

    #[test]
    fn formats() {
        assert_eq!(format_cents(123456), "R$ 1.234,56");
        assert_eq!(format_cents(-5), "-R$ 0,05");
        assert_eq!(format_cents(0), "R$ 0,00");
    }
}
