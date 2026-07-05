use anyhow::{Result, bail};
use std::collections::HashMap;

use super::{NetworkProvider, ProviderSpec, reject_unknown_query};

pub struct SshProvider;

impl NetworkProvider for SshProvider {
    fn scheme(&self) -> &'static str {
        "ssh"
    }

    fn parse(
        &self,
        authority: &str,
        segments: &[&str],
        query: &HashMap<String, String>,
    ) -> Result<ProviderSpec> {
        if segments.len() != 2 {
            bail!("ssh URI must be /<remote-host>/<remote-port>");
        }
        reject_unknown_query(query, &["local"])?;

        let (jump_host, jump_port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), port.parse::<u16>()?),
            None => (authority.to_string(), 22),
        };

        Ok(ProviderSpec::Ssh {
            jump_host,
            jump_port,
            remote_host: segments[0].to_string(),
            remote_port: segments[1].parse::<u16>()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_parse_ssh_uri() {
        let provider = SshProvider;
        let query = HashMap::new();

        // Basic
        let spec = provider
            .parse("jump.host", &["remote.host", "5432"], &query)
            .unwrap();
        if let ProviderSpec::Ssh {
            jump_host,
            jump_port,
            remote_host,
            remote_port,
        } = spec
        {
            assert_eq!(jump_host, "jump.host");
            assert_eq!(jump_port, 22);
            assert_eq!(remote_host, "remote.host");
            assert_eq!(remote_port, 5432);
        } else {
            panic!("Wrong spec type");
        }

        // With jump port
        let spec = provider
            .parse("jump.host:2222", &["remote.host", "5432"], &query)
            .unwrap();
        if let ProviderSpec::Ssh { jump_port, .. } = spec {
            assert_eq!(jump_port, 2222);
        }

        // Rejections
        assert!(
            provider
                .parse("jump.host", &["remote.host"], &query)
                .is_err()
        );
        assert!(
            provider
                .parse("jump.host", &["remote.host", "5432", "extra"], &query)
                .is_err()
        );
        assert!(
            provider
                .parse("jump.host", &["remote.host", "not-a-port"], &query)
                .is_err()
        );

        let mut bad_query = HashMap::new();
        bad_query.insert("unknown".to_string(), "val".to_string());
        assert!(
            provider
                .parse("jump.host", &["remote.host", "5432"], &bad_query)
                .is_err()
        );
    }
}
