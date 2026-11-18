//! Release metadata for this fork.

pub const REPO: &str = "kingof9x/framework-control-windows-naitive";
pub const RELEASES_URL: &str = "https://github.com/kingof9x/framework-control-windows-naitive/releases/latest";
pub const UPSTREAM_URL: &str = "https://github.com/ozturkkl/framework-control";

/// Numeric version comparison ("1.10.0" > "1.9.0"); same rule as the service.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    }
    let (a, b) = (parts(latest), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn newer() {
        assert!(super::is_newer("1.10.0", "1.9.0"));
        assert!(!super::is_newer("1.0.0", "v1.0.0"));
    }
}
