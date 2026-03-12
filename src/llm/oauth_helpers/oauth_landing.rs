//! HTML helpers for the OAuth callback landing page.
//!
//! This module renders the small success or failure page shown after an OAuth
//! callback completes, including basic escaping for provider names.

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// Generate a branded HTML landing page for the OAuth callback result.
pub fn landing_html(provider_name: &str, success: bool) -> String {
    let safe_name = html_escape(provider_name);
    let (icon, heading, subtitle, accent) = if success {
        (
            r##"<div style="width:64px;height:64px;border-radius:50%;background:#22c55e;display:flex;align-items:center;justify-content:center;margin:0 auto 24px">
                <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
              </div>"##,
            format!("{safe_name} Connected"),
            "You can close this window and return to your terminal.",
            "#22c55e",
        )
    } else {
        (
            r##"<div style="width:64px;height:64px;border-radius:50%;background:#ef4444;display:flex;align-items:center;justify-content:center;margin:0 auto 24px">
                <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
              </div>"##,
            "Authorization Failed".to_string(),
            "The request was denied. You can close this window and try again.",
            "#ef4444",
        )
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>IronClaw - {heading}</title>
<style>
  * {{ margin:0; padding:0; box-sizing:border-box }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    background: #0a0a0a;
    color: #e5e5e5;
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 100vh;
  }}
  .card {{
    text-align: center;
    padding: 48px 40px;
    max-width: 420px;
    border: 1px solid #262626;
    border-radius: 16px;
    background: #141414;
  }}
  h1 {{
    font-size: 22px;
    font-weight: 600;
    margin-bottom: 8px;
    color: #fafafa;
  }}
  p {{
    font-size: 14px;
    color: #a3a3a3;
    line-height: 1.5;
  }}
  .accent {{ color: {accent}; }}
  .brand {{
    margin-top: 32px;
    font-size: 12px;
    color: #525252;
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }}
</style>
</head>
<body>
  <div class="card">
    {icon}
    <h1>{heading}</h1>
    <p>{subtitle}</p>
    <div class="brand">IronClaw</div>
  </div>
</body>
</html>"#,
        heading = heading,
        icon = icon,
        subtitle = subtitle,
        accent = accent,
    )
}
