use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum TriggerDef {
    Cron(String),
    Webhook,
    LogPattern(String),
    Manual,
}

impl Serialize for TriggerDef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            TriggerDef::Cron(expr) => serializer.serialize_str(&format!("cron({})", expr)),
            TriggerDef::Webhook => serializer.serialize_str("webhook"),
            TriggerDef::LogPattern(pat) => {
                serializer.serialize_str(&format!("log_pattern({})", pat))
            }
            TriggerDef::Manual => serializer.serialize_str("manual"),
        }
    }
}

impl<'de> Deserialize<'de> for TriggerDef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        parse_trigger(&s).map_err(serde::de::Error::custom)
    }
}

fn parse_trigger(s: &str) -> Result<TriggerDef, String> {
    let trimmed = s.trim();
    if trimmed == "webhook" {
        Ok(TriggerDef::Webhook)
    } else if trimmed == "manual" {
        Ok(TriggerDef::Manual)
    } else if let Some(inner) = trimmed
        .strip_prefix("cron(")
        .and_then(|s| s.strip_suffix(')'))
    {
        Ok(TriggerDef::Cron(inner.trim().to_string()))
    } else if let Some(inner) = trimmed
        .strip_prefix("log_pattern(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let inner = inner.trim().trim_matches('"').trim_matches('\'');
        Ok(TriggerDef::LogPattern(inner.to_string()))
    } else {
        Err(format!("unknown trigger: {}", trimmed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cron_trigger() {
        assert_eq!(
            parse_trigger("cron(0 9 * * *)").unwrap(),
            TriggerDef::Cron("0 9 * * *".to_string())
        );
    }

    #[test]
    fn parse_webhook_trigger() {
        assert_eq!(parse_trigger("webhook").unwrap(), TriggerDef::Webhook);
    }

    #[test]
    fn parse_log_pattern_trigger() {
        assert_eq!(
            parse_trigger("log_pattern(\"ERROR\")").unwrap(),
            TriggerDef::LogPattern("ERROR".to_string())
        );
    }

    #[test]
    fn parse_manual_trigger() {
        assert_eq!(parse_trigger("manual").unwrap(), TriggerDef::Manual);
    }

    #[test]
    fn parse_invalid_trigger() {
        assert!(parse_trigger("unknown").is_err());
    }
}
