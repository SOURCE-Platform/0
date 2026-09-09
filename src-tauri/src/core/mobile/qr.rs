use super::enrollment::EnrollmentPayload;
use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

/// Render the enrollment payload as an SVG QR code for the Settings screen.
pub fn render_enrollment_qr(payload: &EnrollmentPayload) -> Result<String, String> {
    let json = serde_json::to_string(payload)
        .map_err(|error| format!("Failed to encode pairing payload: {error}"))?;
    let code = QrCode::with_error_correction_level(json.as_bytes(), EcLevel::M)
        .map_err(|error| format!("Failed to build QR code: {error}"))?;
    let rendered = code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .dark_color(svg::Color("#1c1917"))
        .light_color(svg::Color("#ffffff"))
        .build();
    // Drop the XML prolog: this is injected inline into the settings DOM, where
    // a processing instruction would not render.
    Ok(match rendered.find("<svg") {
        Some(index) => rendered[index..].to_string(),
        None => rendered,
    })
}

/// Best-effort hostname the phone can reach this Mac on.
pub fn local_hostname() -> String {
    let raw = hostname::get()
        .ok()
        .and_then(|name| name.into_string().ok())
        .unwrap_or_else(|| "source".to_string());
    if raw.ends_with(".local") {
        raw
    } else {
        format!("{raw}.local")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> EnrollmentPayload {
        EnrollmentPayload {
            v: 1,
            host: "example-mac.local".to_string(),
            port: 8787,
            fp: "a".repeat(64),
            secret: "b".repeat(32),
            name: "Example MacBook Air".to_string(),
        }
    }

    #[test]
    fn renders_svg_for_a_full_payload() {
        let svg = render_enrollment_qr(&payload()).expect("render");
        assert!(svg.starts_with("<svg"), "must be inline-injectable: {}", &svg[..40]);
        assert!(!svg.contains("<?xml"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn hostname_is_resolvable_by_bonjour() {
        assert!(local_hostname().ends_with(".local"));
    }
}
