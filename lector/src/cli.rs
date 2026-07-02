#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub engine: String,
    pub probe_terminal: bool,
}

impl Config {
    pub fn parse() -> Self {
        Self::parse_from(std::env::args().skip(1))
    }

    fn parse_from(args: impl IntoIterator<Item = String>) -> Self {
        let mut args = args.into_iter();
        let mut engine = default_engine().to_string();
        let mut url = None;
        let mut probe_terminal = false;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--engine" => {
                    if let Some(value) = args.next() {
                        engine = value;
                    }
                }
                "--servo" => engine = "servo-native".to_string(),
                "--probe-terminal" => probe_terminal = true,
                value => url = Some(value.to_string()),
            }
        }

        Self {
            url: url.unwrap_or_else(|| "https://example.com".to_string()),
            engine,
            probe_terminal,
        }
    }
}

fn default_engine() -> &'static str {
    if cfg!(feature = "servo-native") {
        "servo-native"
    } else {
        "html"
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, default_engine};

    #[test]
    fn parses_default_url_and_engine() {
        let config = Config::parse_from([]);
        assert_eq!(config.url, "https://example.com");
        assert_eq!(config.engine, default_engine());
        assert!(!config.probe_terminal);
    }

    #[test]
    fn parses_servo_native_alias() {
        let config = Config::parse_from(["--servo", "https://servo.org"].map(String::from));
        assert_eq!(config.url, "https://servo.org");
        assert_eq!(config.engine, "servo-native");
    }

    #[test]
    fn parses_terminal_probe() {
        let config = Config::parse_from(["--probe-terminal"].map(String::from));
        assert!(config.probe_terminal);
    }
}
