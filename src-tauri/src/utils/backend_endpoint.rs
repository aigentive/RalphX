pub const PRODUCTION_BACKEND_PORT: u16 = 3847;
pub const DEVELOPMENT_BACKEND_PORT: u16 = 3857;
pub const BACKEND_PORT_ENV: &str = "RALPHX_BACKEND_PORT";

pub fn backend_http_port() -> u16 {
    let default_port = default_backend_port();
    match std::env::var(BACKEND_PORT_ENV) {
        Ok(value) => match parse_backend_port(Some(value.as_str()), default_port) {
            Ok(port) => port,
            Err(error) => {
                tracing::warn!(
                    env_var = BACKEND_PORT_ENV,
                    value = %value,
                    default_port,
                    error = %error,
                    "Ignoring invalid backend port override"
                );
                default_port
            }
        },
        Err(_) => default_port,
    }
}

pub fn backend_http_base_url() -> String {
    format!("http://127.0.0.1:{}", backend_http_port())
}

pub fn backend_http_bind_addr() -> String {
    format!("127.0.0.1:{}", backend_http_port())
}

fn default_backend_port() -> u16 {
    if cfg!(debug_assertions) {
        DEVELOPMENT_BACKEND_PORT
    } else {
        PRODUCTION_BACKEND_PORT
    }
}

pub(crate) fn parse_backend_port(
    raw: Option<&str>,
    default_port: u16,
) -> Result<u16, &'static str> {
    let Some(raw) = raw else {
        return Ok(default_port);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty value");
    }
    let port = trimmed.parse::<u16>().map_err(|_| "not a valid u16 port")?;
    if port == 0 {
        return Err("port must be greater than zero");
    }
    Ok(port)
}
