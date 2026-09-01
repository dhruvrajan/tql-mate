use crate::{Error, Result};

/// Parsed `typedb://user:pass@host:port/database[?tls=…]` connection URL.
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
        let rest = raw.strip_prefix("typedb://").ok_or(Error::UrlScheme)?;

        let (auth, after_auth) = rest.split_once('@').ok_or(Error::UrlAuthHost)?;
        let (username, password) = auth.split_once(':').ok_or(Error::UrlAuthPair)?;

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
                let port: u16 = p.parse().map_err(|_| Error::UrlPort(p.to_string()))?;
                (h.to_string(), port)
            }
            None => (hostport.to_string(), 1729),
        };

        let database = path.trim_matches('/').to_string();
        if database.is_empty() {
            return Err(Error::UrlDatabase);
        }

        let mut tls = false;
        for pair in query.split('&').filter(|s| !s.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if k == "tls" {
                tls = matches!(v, "true" | "1" | "yes");
            }
        }

        Ok(Self {
            username: percent_decode(username),
            password: percent_decode(password),
            host,
            port,
            database,
            tls,
        })
    }

    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Clone with a different database name (integration tests use unique DBs).
    pub fn with_database(&self, database: impl Into<String>) -> Self {
        Self {
            database: database.into(),
            ..self.clone()
        }
    }
}

fn percent_decode(s: &str) -> String {
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
    fn table_driven_parse() {
        let ok_cases = [
            (
                "typedb://admin:secret@db.example:1730/mydb?tls=true",
                TypeDbUrl {
                    username: "admin".into(),
                    password: "secret".into(),
                    host: "db.example".into(),
                    port: 1730,
                    database: "mydb".into(),
                    tls: true,
                },
            ),
            (
                "typedb://admin:password@localhost/app",
                TypeDbUrl {
                    username: "admin".into(),
                    password: "password".into(),
                    host: "localhost".into(),
                    port: 1729,
                    database: "app".into(),
                    tls: false,
                },
            ),
            (
                "typedb://user:p%40ss@localhost/db",
                TypeDbUrl {
                    username: "user".into(),
                    password: "p@ss".into(),
                    host: "localhost".into(),
                    port: 1729,
                    database: "db".into(),
                    tls: false,
                },
            ),
            (
                "typedb://u:p+word@127.0.0.1:1729/x?tls=1",
                TypeDbUrl {
                    username: "u".into(),
                    password: "p word".into(),
                    host: "127.0.0.1".into(),
                    port: 1729,
                    database: "x".into(),
                    tls: true,
                },
            ),
            (
                "typedb://u:p@host/db?tls=yes",
                TypeDbUrl {
                    username: "u".into(),
                    password: "p".into(),
                    host: "host".into(),
                    port: 1729,
                    database: "db".into(),
                    tls: true,
                },
            ),
            (
                "typedb://u:p@host/db?tls=false",
                TypeDbUrl {
                    username: "u".into(),
                    password: "p".into(),
                    host: "host".into(),
                    port: 1729,
                    database: "db".into(),
                    tls: false,
                },
            ),
            (
                "typedb://u:p@host/nested/path",
                TypeDbUrl {
                    username: "u".into(),
                    password: "p".into(),
                    host: "host".into(),
                    port: 1729,
                    database: "nested/path".into(),
                    tls: false,
                },
            ),
        ];
        for (raw, expect) in ok_cases {
            assert_eq!(TypeDbUrl::parse(raw).unwrap(), expect, "parse({raw:?})");
        }

        let err_cases = [
            ("http://admin:x@localhost/db", "UrlScheme"),
            ("typedb://localhost/db", "UrlAuthHost"),
            ("typedb://admin@localhost/db", "UrlAuthPair"),
            ("typedb://admin:pass@localhost", "UrlDatabase"),
            ("typedb://admin:pass@localhost/", "UrlDatabase"),
            ("typedb://admin:pass@localhost:xyz/db", "UrlPort"),
        ];
        for (raw, kind) in err_cases {
            let err = TypeDbUrl::parse(raw).expect_err(raw);
            let label = format!("{err:?}");
            assert!(label.contains(kind), "parse({raw:?}) => {err:?}");
        }
    }

    #[test]
    fn address_uses_host_port() {
        let u = TypeDbUrl::parse("typedb://a:b@db.example:1730/mydb").unwrap();
        assert_eq!(u.address(), "db.example:1730");
    }

    #[test]
    fn with_database_preserves_other_fields() {
        let u = TypeDbUrl::parse("typedb://a:b@h:1/old?tls=true").unwrap();
        let n = u.with_database("new_db");
        assert_eq!(n.database, "new_db");
        assert_eq!(n.host, "h");
        assert!(n.tls);
    }
}
