//! Embedded dashboard assets — mirrors Go `//go:embed` references.

pub const PAGE_HTML: &str = include_str!("../../../internal/dashboard/page.html");
pub const ICON_PNG: &[u8] = include_bytes!("../../../internal/dashboard/icon.png");
