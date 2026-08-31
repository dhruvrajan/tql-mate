use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDbUrl {
    pub username: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub tls: bool,
}

impl TypeDbUrl {
    pub fn parse(raw: &str) -> Result<Self> {
        let rest = raw
            .strip_prefix("typedb://")
            .ok_or_else(|| Error::msg("URL must start with typedb://"))?;

        let (auth, after_auth) = rest
            .split_once('@')
            .ok_or_else(|| Error::msg("URL missing user:pass@host"))?;
        let (username, password) = auth
            .split_once(':')
            .ok_or_else(|| Error::msg("URL auth must be user:pass"))?;

        let (hostport, path_query) = match after_auth.split_once('/') {
            Some((hp, pq)) => (hp, pq),
            None => (after_auth, ""),
        };
        let (path, query) = match path_query.split_once('?') {
            Some((p, q)) => (p, q),
            None => (path_query, ""),
        };

        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => {
                let port: u16 = p
                    .parse()
                    .map_err(|_| Error::msg(format!("invalid port: {p}")))?;
                (h.to_string(), port)
            }
            None => (hostport.to_string(), 1729),
        };

        let database = path.trim_matches('/').to_string();
        if database.is_empty() {
            return Err(Error::msg("URL must include /database"));
        }

        let mut tls = false;
        for pair in query.split('&').filter(|s| !s.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if k == "tls" {
                tls = matches!(v, "true" | "1" | "yes");
            }
        }

        Ok(Self {
            username: decode(username),
            password: decode(password),
            host,
            port,
            database,
            tls,
        })
    }

    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(a), Some(b)) = (h1, h2) {
                if let Ok(byte) = u8::from_str_radix(&format!("{a}{b}"), 16) {
                    out.push(byte as char);
                    continue;
                }
            }
            out.push('%');
            if let Some(a) = h1 {
                out.push(a);
            }
            if let Some(b) = h2 {
                out.push(b);
            }
        } else if c == '+' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_url() {
        let u = TypeDbUrl::parse("typedb://admin:secret@db.example:1730/mydb?tls=true").unwrap();
        assert_eq!(u.username, "admin");
        assert_eq!(u.password, "secret");
        assert_eq!(u.host, "db.example");
        assert_eq!(u.port, 1730);
        assert_eq!(u.database, "mydb");
        assert!(u.tls);
        assert_eq!(u.address(), "db.example:1730");
    }

    #[test]
    fn default_port_and_no_tls() {
        let u = TypeDbUrl::parse("typedb://admin:password@localhost/app").unwrap();
        assert_eq!(u.port, 1729);
        assert!(!u.tls);
    }

    #[test]
    fn percent_encoded_password() {
        let u = TypeDbUrl::parse("typedb://user:p%40ss@localhost/db").unwrap();
        assert_eq!(u.password, "p@ss");
    }

    #[test]
    fn rejects_bad_scheme() {
        assert!(TypeDbUrl::parse("http://admin:x@localhost/db").is_err());
    }
}
