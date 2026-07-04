//! Upstream failover helpers.

use reqwest::StatusCode;

pub fn should_failover(status: StatusCode, network_err: bool) -> bool {
    if network_err {
        return true;
    }
    matches!(
        status.as_u16(),
        429 | 502 | 503 | 504
    )
}
