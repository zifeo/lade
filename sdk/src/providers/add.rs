#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddField {
    pub key: &'static str,
    pub prompt: &'static str,
    pub default: Option<&'static str>,
}

pub fn secret_search_scope(scheme: &str) -> Option<AddField> {
    match scheme {
        "azurekv" => Some(super::azurekv::SEARCH_SCOPE),
        _ => None,
    }
}

pub fn secret_add_fields(scheme: &str) -> &'static [AddField] {
    match scheme {
        "op" => super::onepassword::ADD_FIELDS,
        "vault" => super::vault::ADD_FIELDS,
        "awssm" => super::awssm::ADD_FIELDS,
        "azurekv" => super::azurekv::ADD_FIELDS,
        "gcpsm" => super::gcpsm::ADD_FIELDS,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_asks_field_and_host() {
        let fields = secret_add_fields("vault");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].key, "field");
        assert_eq!(fields[1].key, "host");
        assert_eq!(fields[1].default, Some("127.0.0.1:8200"));
    }

    #[test]
    fn azurekv_search_needs_vault() {
        let scope = secret_search_scope("azurekv").expect("scope");
        assert_eq!(scope.key, "vault");
        assert!(secret_search_scope("op").is_none());
    }
}
