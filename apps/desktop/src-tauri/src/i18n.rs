//! Textos do lado nativo (a bandeja), do mesmo catálogo da interface.

use std::sync::LazyLock;

use serde_json::Value;

static CATALOG: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../packages/i18n/locales/pt-BR.json"))
        .expect("o catálogo pt-BR é JSON válido")
});

/// O texto da chave (`"tray.open"`). Uma chave sem texto volta como está,
/// para aparecer na tela em vez de sumir; o check do catálogo pega isso antes.
pub fn tr(key: &str) -> String {
    key.split('.')
        .try_fold(&*CATALOG, |node, part| node.get(part))
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acha_texto_aninhado_e_devolve_a_chave_quando_falta() {
        assert_eq!(tr("tray.quit"), "Sair do Solune");
        // Por variável, para o check do catálogo não contar como uso.
        for missing in ["tray.nao_existe", "tray"] {
            assert_eq!(tr(missing), missing);
        }
    }
}
