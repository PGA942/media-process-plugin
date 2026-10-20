//! 利用者に見せる文の表(PROTOCOL.md §4.6)。ホストの `args.locale`(無ければ `ja`)で引く。
//!
//! 基準は `ja`。`en` / `zh-Hans` / `zh-Hant` / `ko` は同じキーを全部持つ(テストで確かめる)。

rasq_plugin_kit::i18n_catalog! {
    pub(crate) static TEXT = {
        "ja": {
            "action.sticker" => "スタンプ",
            "action.reset" => "リセット",
            "overlay.face" => "顔 {n}",
            "overlay.sticker" => "スタンプ {n}",
            "sticker.title" => "スタンプ",
            "sticker.close" => "閉じる",
            "sticker.smile" => "スマイル",
            "sticker.heart" => "ハート",
            "sticker.star" => "スター",
            "faces.notFound" => "顔が見つかりませんでした",
        },
        "en": {
            "action.sticker" => "Sticker",
            "action.reset" => "Reset",
            "overlay.face" => "Face {n}",
            "overlay.sticker" => "Sticker {n}",
            "sticker.title" => "Stickers",
            "sticker.close" => "Close",
            "sticker.smile" => "Smile",
            "sticker.heart" => "Heart",
            "sticker.star" => "Star",
            "faces.notFound" => "No faces found",
        },
        "zh-Hans": {
            "action.sticker" => "贴纸",
            "action.reset" => "重置",
            "overlay.face" => "人脸 {n}",
            "overlay.sticker" => "贴纸 {n}",
            "sticker.title" => "贴纸",
            "sticker.close" => "关闭",
            "sticker.smile" => "笑脸",
            "sticker.heart" => "爱心",
            "sticker.star" => "星星",
            "faces.notFound" => "未检测到人脸",
        },
        "zh-Hant": {
            "action.sticker" => "貼圖",
            "action.reset" => "重設",
            "overlay.face" => "人臉 {n}",
            "overlay.sticker" => "貼圖 {n}",
            "sticker.title" => "貼圖",
            "sticker.close" => "關閉",
            "sticker.smile" => "笑臉",
            "sticker.heart" => "愛心",
            "sticker.star" => "星星",
            "faces.notFound" => "未偵測到人臉",
        },
        "ko": {
            "action.sticker" => "스티커",
            "action.reset" => "초기화",
            "overlay.face" => "얼굴 {n}",
            "overlay.sticker" => "스티커 {n}",
            "sticker.title" => "스티커",
            "sticker.close" => "닫기",
            "sticker.smile" => "스마일",
            "sticker.heart" => "하트",
            "sticker.star" => "별",
            "faces.notFound" => "얼굴을 찾지 못했습니다",
        },
    };
}

#[cfg(test)]
mod tests {
    use super::TEXT;
    use std::collections::BTreeSet;

    /// この表の言語ごとのキー(`Catalog::check` は「基準に無いキー」と差し込みの食い違いしか見ないので、訳の抜けはここで見る)。
    fn table_keys() -> Vec<(String, BTreeSet<String>)> {
        let mut out: Vec<(String, BTreeSet<String>)> = Vec::new();
        for line in include_str!("text.rs").lines().map(str::trim) {
            if let Some(tag) = line.strip_prefix('"').and_then(|l| l.strip_suffix("\": {")) {
                out.push((tag.to_string(), BTreeSet::new()));
            } else if let Some((key, _)) = line.strip_prefix('"').and_then(|l| l.split_once("\" =>")) {
                out.last_mut().expect("表の外のキー").1.insert(key.to_string());
            }
        }
        out
    }

    #[test]
    fn every_locale_has_every_key() {
        assert!(TEXT.check().is_empty(), "{:?}", TEXT.check());
        let tables = table_keys();
        let tags: Vec<&str> = tables.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(tags, TEXT.locales());
        assert_eq!(tags, ["ja", "en", "zh-Hans", "zh-Hant", "ko"]);
        for (tag, keys) in &tables[1..] {
            let missing: Vec<_> = tables[0].1.difference(keys).collect();
            assert!(missing.is_empty(), "{tag} に訳が無いキー: {missing:?}");
        }
    }
}
