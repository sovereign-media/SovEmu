//! Text as the library compares it.

/// Lower case without accents ("Pokémon" → "pokemon"), as the clients fold for their search.
/// Covers Latin-1 and Latin Extended-A, which is what game titles use.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        for lower in ch.to_lowercase() {
            match lower {
                'à'..='å' | 'ā' | 'ă' | 'ą' => out.push('a'),
                'æ' => out.push_str("ae"),
                'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => out.push('c'),
                'ď' | 'đ' => out.push('d'),
                'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => out.push('e'),
                'ĝ' | 'ğ' | 'ġ' | 'ģ' => out.push('g'),
                'ĥ' | 'ħ' => out.push('h'),
                'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => out.push('i'),
                'ĵ' => out.push('j'),
                'ķ' => out.push('k'),
                'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => out.push('l'),
                'ñ' | 'ń' | 'ņ' | 'ň' => out.push('n'),
                'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => out.push('o'),
                'œ' => out.push_str("oe"),
                'ŕ' | 'ŗ' | 'ř' => out.push('r'),
                'ś' | 'ŝ' | 'ş' | 'š' => out.push('s'),
                'ß' => out.push_str("ss"),
                'ţ' | 'ť' | 'ŧ' => out.push('t'),
                'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => out.push('u'),
                'ŵ' => out.push('w'),
                'ý' | 'ÿ' | 'ŷ' => out.push('y'),
                'ź' | 'ż' | 'ž' => out.push('z'),
                other => out.push(other),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case_and_accents() {
        assert_eq!(fold("Pokémon Café ÆON Straße"), "pokemon cafe aeon strasse");
    }
}
