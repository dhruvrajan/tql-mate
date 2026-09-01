use crate::{Error, Result};

/// Credentials assumed when the URL omits `user:pass@`. These match TypeDB CE.
pub const DEFAULT_USERNAME: &str = "admin";
pub const DEFAULT_PASSWORD: &str = "password";

/// Parsed `typedb://[user:pass@]host[:port]/database[?tls=…]` connection URL.
///
/// `user:pass@` is optional and defaults to [`DEFAULT_USERNAME`] /
/// [`DEFAULT_PASSWORD`]; the port defaults to 1729. The database is required —
/// `create` and `drop` act on whatever it names, so it is never guessed.
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
            .ok_or_else(|| Error::UrlScheme(raw.to_string()))?;

        // Split on the last '@' so a literal '@' in the password still parses.
        let (username, password, after_auth) = match rest.rsplit_once('@') {
            Some((auth, after)) => {
                let (u, p) = auth.split_once(':').ok_or(Error::UrlAuthPair)?;
                (u, p, after)
            }
            None => (DEFAULT_USERNAME, DEFAULT_PASSWORD, rest),
        };

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
