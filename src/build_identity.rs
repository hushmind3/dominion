//! Immutable revision recorded when Rust was compiled. Never substitute
//! the HEAD of a checkout that may move while an older binary is running.
use std::sync::OnceLock;

pub struct Identity {
    pub commit: String,
    pub branch: String,
    pub dirty: Option<bool>,
    pub source: &'static str,
}

pub fn running() -> &'static Identity {
    static BUILD:OnceLock<Identity>=OnceLock::new();
    BUILD.get_or_init(||{
        let commit=option_env!("DOMINION_BUILD_COMMIT")
            .filter(|v|v.len()==40&&v.bytes().all(|b|b.is_ascii_hexdigit()));
        match commit {
            Some(commit)=>Identity {
                commit:commit.to_owned(),
                branch:option_env!("DOMINION_BUILD_BRANCH")
                    .unwrap_or("unknown").to_owned(),
                dirty:option_env!("DOMINION_BUILD_DIRTY")
                    .and_then(|v|v.parse::<bool>().ok()),
                source:match option_env!("DOMINION_BUILD_IDENTITY_SOURCE") {
                    Some("embedded-release")=>"embedded-release",
                    Some("embedded-build")=>"embedded-build",
                    _=>"embedded-build",
                },
            },
            None=>Identity {
                commit:"unknown".into(),branch:"unknown".into(),
                dirty:None,source:"unavailable",
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn health_identity_is_from_binary_or_explicitly_unknown(){
        let identity=running();
        assert!(matches!(identity.source,
            "embedded-release"|"embedded-build"|"unavailable"));
        if identity.source=="unavailable" {
            assert_eq!(identity.commit,"unknown");
        }else{
            assert_eq!(identity.commit.len(),40);
            assert!(identity.commit.bytes().all(|b|b.is_ascii_hexdigit()));
        }
        assert_ne!(identity.source,"source-checkout",
            "a moving Git worktree is not evidence of the running binary");
    }
}
