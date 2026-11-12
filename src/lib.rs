//! 把教材資料夾裡的 `.txt` 讀成一張張單字卡。
//!
//! 手機上的網頁不會執行這個檔案。電腦下
//! `cargo run -- --export docs --no-open` 時，`main.rs` 會呼叫
//! [`load_decks`]，再把結果寫成 `cards.json`。
//!
//! 一個 `.txt` 就是一份 [`Deck`]。檔名（例如 `daily.txt`）記在每張卡的
//! `source`。只有文字檔會讀，履歷那種 `.doc` 會跳過。
//! 檔案可以是 UTF-8，也可以是 Big5；開頭若有 UTF-8 的 BOM 會先拿掉。

use std::fs;
use std::path::Path;

/// 畫面上的一張卡。空白欄位用空字串，不用「沒有值」。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    /// 要念出來的英文。片語中間的空白會保留，例如 `stick to`。
    pub english: String,
    /// 拆開的音節，顯示在單字上方。沒有就留空。例如 `im pre ssive`。
    pub syllables: String,
    /// 括號裡的讀音提示，例如 `趴里港`。跟音節相同時，網頁不另外顯示。
    pub hint: String,
    /// 中文意思。同一張卡有多行中文時，用換行接在一起。
    pub meaning: String,
    /// 補充。整行括號是註記；同一個空行區塊裡的其他英文會寫成「同組：…」。
    pub note: String,
    /// 現在這個段落的標題，例如 `情緒` 或 `+ of`。
    pub section: String,
    /// 這張卡來自哪一個檔名。
    pub source: String,
}

/// 一份教材。`name` 是檔名，含 `.txt`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deck {
    pub name: String,
    pub cards: Vec<Card>,
}

/// 區塊裡讀到的英文，還沒跟中文拼成 [`Card`]。
struct Piece {
    english: String,
    syllables: String,
    hint: String,
}

/// 讀資料夾裡每一個 `.txt`，依檔名排序。讀不到任何單字的檔案不會變成教材。
pub fn load_decks(dir: &Path) -> std::io::Result<Vec<Deck>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".txt") {
            continue;
        }
        files.push((name.to_string(), path));
    }
    files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    let mut decks = Vec::new();
    for (name, path) in files {
        let text = decode_bytes(&fs::read(&path)?);
        let cards = parse_document(&name, &text);
        if !cards.is_empty() {
            decks.push(Deck { name, cards });
        }
    }
    Ok(decks)
}

/// 把一份教材的全文變成卡片。
///
/// `prefix.txt` 這種用 Tab 縮排的字首樹，跟一般「空行分隔的單字」走不同的讀法。
/// 超過一半的非空白行都以 Tab 開頭時，當成字首樹。
pub fn parse_document(source: &str, text: &str) -> Vec<Card> {
    let text = text.trim_start_matches('\u{feff}');
    if is_prefix_tree(text) {
        parse_prefix(source, text)
    } else {
        parse_blocks(source, text)
    }
}

/// UTF-8（可含 BOM）優先。不是合法 UTF-8 時，用 Big5 再解一次。
pub fn decode_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => encoding_rs::BIG5.decode(bytes).0.into_owned(),
    }
}

/// 一般教材：空行或一排橫線結束一張卡。
///
/// 段落標題要逐行判斷。有些檔案在標題下一行立刻寫單字，中間沒有空行。
fn parse_blocks(source: &str, text: &str) -> Vec<Card> {
    let mut section = String::new();
    let mut cards = Vec::new();
    let mut current = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_rule(trimmed) {
            flush_block(source, &section, &mut current, &mut cards);
            continue;
        }
        if let Some(title) = section_line(trimmed) {
            flush_block(source, &section, &mut current, &mut cards);
            section = title;
            continue;
        }
        current.push(trimmed.to_string());
    }
    flush_block(source, &section, &mut current, &mut cards);
    cards
}

fn flush_block(source: &str, section: &str, current: &mut Vec<String>, cards: &mut Vec<Card>) {
    if current.is_empty() {
        return;
    }
    cards.extend(cards_from_block(source, section, current));
    current.clear();
}

fn is_rule(line: &str) -> bool {
    let line = line.trim();
    line.chars().count() >= 3
        && line
            .chars()
            .all(|ch| matches!(ch, '-' | '—' | '─' | '=' | '*' | '－'))
}

fn section_line(line: &str) -> Option<String> {
    if let Some(title) = dashed_title(line) {
        return Some(title);
    }
    if is_plus_label(line) {
        return Some(line.trim().to_string());
    }
    None
}

fn dashed_title(line: &str) -> Option<String> {
    let dashes = line
        .chars()
        .filter(|ch| matches!(ch, '-' | '—' | '─' | '=' | '－'))
        .count();
    if dashes < 3 {
        return None;
    }
    let inner: String = line
        .chars()
        .filter(|ch| !matches!(ch, '-' | '—' | '─' | '=' | '－'))
        .collect();
    let inner = inner.trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

fn is_plus_label(line: &str) -> bool {
    let line = line.trim();
    if has_cjk(line) || line.contains('、') {
        return false;
    }
    let marked = line.starts_with("+ ") || line.ends_with(" +") || line == "+";
    if !marked {
        return false;
    }
    let core = line.trim_matches('+').trim();
    !core.is_empty()
        && core
            .chars()
            .all(|ch| ch.is_ascii_alphabetic() || matches!(ch, ' ' | '-' | '\''))
}

fn cards_from_block(source: &str, section: &str, lines: &[String]) -> Vec<Card> {
    let mut pieces = Vec::new();
    let mut meanings = Vec::new();
    let mut notes = Vec::new();

    for line in lines {
        if is_note(line) {
            notes.push(unwrap_note(line));
            continue;
        }
        // 括號要先於「英文接中文」拆開。否則 `poly gon(趴里港)` 會把
        // 左括號留在英文裡，音節就變成 `poly gon(`。
        let (_, hint) = split_paren(line);
        if !hint.is_empty() {
            if let Some(piece) = piece_from_english(line) {
                pieces.push(piece);
            }
            continue;
        }
        if let Some((english, chinese)) = split_mixed(line) {
            if let Some(piece) = piece_from_english(&english) {
                pieces.push(piece);
            }
            if !chinese.is_empty() {
                meanings.push(chinese);
            }
            continue;
        }
        if is_english_line(line) {
            let (base, hint) = split_paren(line);
            if base.contains('、') {
                for part in base.split('、') {
                    if let Some(mut piece) = piece_from_english(part) {
                        if piece.hint.is_empty() {
                            piece.hint = hint.clone();
                        }
                        pieces.push(piece);
                    }
                }
            } else if let Some(piece) = piece_from_english(line) {
                pieces.push(piece);
            }
            continue;
        }
        let meaning = line.trim();
        if !meaning.is_empty() {
            meanings.push(meaning.to_string());
        }
    }

    // `im pre ssive` 下一行若是同一個字 `impressive`，上一行改當音節。
    merge_syllable_pair(&mut pieces);
    // 區塊裡已經有真正的單字時，丟掉單獨一個字母，例如意思後面多出來的 `s`。
    if pieces.iter().any(|piece| piece.english.chars().count() > 1) {
        pieces.retain(|piece| piece.english.chars().count() > 1);
    }

    if pieces.is_empty() {
        return Vec::new();
    }

    let meaning = meanings.join("\n");
    let note = notes.join("\n");
    let siblings: Vec<String> = pieces.iter().map(|piece| piece.english.clone()).collect();

    pieces
        .into_iter()
        .enumerate()
        .filter_map(|(index, piece)| {
            if piece.english.is_empty() {
                return None;
            }
            let group_note = if siblings.len() > 1 {
                let others = siblings
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .map(|(_, word)| word.as_str())
                    .collect::<Vec<_>>()
                    .join("、");
                format!("同組：{others}")
            } else {
                String::new()
            };
            let note = match (note.is_empty(), group_note.is_empty()) {
                (true, true) => String::new(),
                (false, true) => note.clone(),
                (true, false) => group_note,
                (false, false) => format!("{note}\n{group_note}"),
            };
            Some(Card {
                english: piece.english,
                syllables: piece.syllables,
                hint: piece.hint,
                meaning: meaning.clone(),
                note,
                section: section.to_string(),
                source: source.to_string(),
            })
        })
        .collect()
}

fn merge_syllable_pair(pieces: &mut Vec<Piece>) {
    if pieces.len() < 2 {
        return;
    }
    let first_split = pieces[0].english.contains(' ') || !pieces[0].syllables.is_empty();
    let first_key = key(&pieces[0].english);
    if !first_split || first_key.len() < 2 || first_key != key(&pieces[1].english) {
        return;
    }
    let syllables = if pieces[0].syllables.is_empty() {
        pieces[0].english.clone()
    } else {
        pieces[0].syllables.clone()
    };
    let hint = if !pieces[0].hint.is_empty() {
        pieces[0].hint.clone()
    } else {
        pieces[1].hint.clone()
    };
    pieces.remove(0);
    pieces[0].syllables = syllables;
    if pieces[0].hint.is_empty() {
        pieces[0].hint = hint;
    }
}

fn piece_from_english(raw: &str) -> Option<Piece> {
    let (base, hint) = split_paren(raw);
    let base = clean_spaces(&base);
    if !base.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    if !hint.is_empty() && base.contains(' ') {
        let english: String = base.chars().filter(|ch| !ch.is_whitespace()).collect();
        if english.is_empty() {
            return None;
        }
        return Some(Piece {
            english,
            syllables: base,
            hint,
        });
    }
    Some(Piece {
        english: base,
        syllables: String::new(),
        hint,
    })
}

fn split_paren(line: &str) -> (String, String) {
    let line = line.trim();
    for (open, close) in [('(', ')'), ('（', '）')] {
        if let Some(start) = line.rfind(open) {
            if line.ends_with(close) && start > 0 {
                let base = line[..start].trim();
                let hint = line[start + open.len_utf8()..line.len() - close.len_utf8()].trim();
                if !base.is_empty() && !has_cjk(base) {
                    return (base.to_string(), hint.to_string());
                }
            }
        }
    }
    (line.to_string(), String::new())
}

fn is_note(line: &str) -> bool {
    let line = line.trim();
    (line.starts_with('(') && line.ends_with(')') && line.chars().count() > 2)
        || (line.starts_with('（') && line.ends_with('）') && line.chars().count() > 2)
}

fn unwrap_note(line: &str) -> String {
    let line = line.trim();
    line.trim_start_matches(['(', '（'])
        .trim_end_matches([')', '）'])
        .trim()
        .to_string()
}

fn split_mixed(line: &str) -> Option<(String, String)> {
    let chars: Vec<char> = line.chars().collect();
    let pos = chars.iter().position(|ch| is_cjk(*ch))?;
    if pos == 0 {
        return None;
    }
    let english: String = chars[..pos].iter().collect();
    let chinese: String = chars[pos..].iter().collect();
    let english = english
        .trim()
        .trim_matches(|ch: char| matches!(ch, '、' | ',' | ':' | '：'))
        .trim();
    if !english.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    Some((english.to_string(), chinese.trim().to_string()))
}

fn is_english_line(line: &str) -> bool {
    let (base, _) = split_paren(line);
    !has_cjk(&base) && base.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn is_prefix_tree(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() < 4 {
        return false;
    }
    let tabbed = lines.iter().filter(|line| line.starts_with('\t')).count();
    tabbed * 2 > lines.len()
}

/// 字首樹。有下一層縮排的那一行是分類，不是單字。
/// 最末層用頓號或逗號切開，每個詞各成一張卡，意思寫成「字首 ab · abi」。
fn parse_prefix(source: &str, text: &str) -> Vec<Card> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let mut cards = Vec::new();
    let mut section = String::new();
    for index in 0..lines.len() {
        let indent = indent_of(lines[index]);
        let content = lines[index].trim();
        let next_indent = lines
            .get(index + 1)
            .map(|line| indent_of(line))
            .unwrap_or(0);
        if next_indent > indent {
            if indent == 0 {
                section = content.to_string();
            }
            continue;
        }
        let path = ancestor_path(&lines, index);
        let meaning = if path.is_empty() {
            String::new()
        } else {
            format!("字首 {}", path.join(" · "))
        };
        for word in leaf_words(content) {
            cards.push(Card {
                english: word,
                syllables: String::new(),
                hint: String::new(),
                meaning: meaning.clone(),
                note: String::new(),
                section: section.clone(),
                source: source.to_string(),
            });
        }
    }
    cards
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == '\t').count()
}

fn ancestor_path(lines: &[&str], index: usize) -> Vec<String> {
    let mut want = indent_of(lines[index]);
    let mut found = Vec::new();
    if want == 0 {
        return found;
    }
    for line in lines[..index].iter().rev() {
        let indent = indent_of(line);
        if indent < want {
            found.push(line.trim().to_string());
            want = indent;
            if want == 0 {
                break;
            }
        }
    }
    found.reverse();
    found
}

fn leaf_words(content: &str) -> Vec<String> {
    content
        .split(['、', ',', '，'])
        .map(|part| part.trim().trim_end_matches(['.', '…']).trim())
        .filter(|part| {
            part.chars().count() >= 3
                && !has_cjk(part)
                && part.chars().any(|ch| ch.is_ascii_alphabetic())
        })
        .map(clean_spaces)
        .collect()
}

fn clean_spaces(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn has_cjk(value: &str) -> bool {
    value.chars().any(is_cjk)
}

fn is_cjk(ch: char) -> bool {
    ('\u{3400}'..='\u{9fff}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn english<'a>(cards: &'a [Card], word: &str) -> &'a Card {
        cards
            .iter()
            .find(|card| card.english == word)
            .unwrap_or_else(|| panic!("missing {word}"))
    }

    #[test]
    fn parses_syllable_word_and_meaning() {
        let cards = parse_document(
            "daily.txt",
            "--------- 情緒 ---------\nim pre ssive\nimpressive\n印象深刻的adj\n\nsorrowful\n傷心的\n(mournful憂傷的)\n\nsphere\n球體n\ns\n",
        );
        let impressive = english(&cards, "impressive");
        assert_eq!(impressive.syllables, "im pre ssive");
        assert_eq!(impressive.section, "情緒");
        assert!(impressive.meaning.contains("印象深刻"));
        assert!(impressive.note.is_empty());

        let sorrowful = english(&cards, "sorrowful");
        assert!(sorrowful.note.contains("mournful"));
        assert!(cards.iter().all(|card| card.english != "s"));
        assert_eq!(english(&cards, "sphere").meaning, "球體n");
    }

    #[test]
    fn keeps_phrases_and_compacts_hinted_syllables() {
        let cards = parse_document(
            "daily.txt",
            "square bracket\n中括弧\n\nnumera tor(nu mer ra tor)\n分子n\n\npoly gon(趴里港)\npolygon\n多角形\n\nbeige(被ge)\n米色n\n\nstick to\n忠於、堅持、固守\n",
        );
        assert_eq!(english(&cards, "square bracket").meaning, "中括弧");
        assert!(english(&cards, "square bracket").syllables.is_empty());

        let numerator = english(&cards, "numerator");
        assert_eq!(numerator.syllables, "numera tor");
        assert_eq!(numerator.hint, "nu mer ra tor");
        assert!(numerator.meaning.contains("分子"));

        let polygon = english(&cards, "polygon");
        assert_eq!(polygon.syllables, "poly gon");
        assert_eq!(polygon.hint, "趴里港");

        assert_eq!(english(&cards, "beige").hint, "被ge");
        assert!(english(&cards, "beige").syllables.is_empty());
        assert_eq!(english(&cards, "stick to").english, "stick to");
    }

    #[test]
    fn parses_sections_synonyms_and_mixed_lines() {
        let cards = parse_document(
            "sample.txt",
            "-------------------------------\n+ of\n-------------------------------\ndeprive of\n剝奪、使喪失\n\nsome +\n\nsomehow不知怎麼地\n\nhence、accordingly\n因此\n\npostpone\nadjourn\n",
        );
        assert_eq!(english(&cards, "deprive of").section, "+ of");
        assert!(english(&cards, "deprive of").meaning.contains("剝奪"));
        assert_eq!(english(&cards, "somehow").section, "some +");
        assert!(english(&cards, "somehow").meaning.contains("不知怎麼地"));
        assert_eq!(english(&cards, "hence").meaning, "因此");
        assert!(english(&cards, "hence").note.contains("accordingly"));
        assert!(english(&cards, "adjourn").note.contains("postpone"));
    }

    #[test]
    fn parses_prefix_tree() {
        let cards = parse_document(
            "prefix.txt",
            "ab\n\tabi\n\t\tabide、ability\n\tafr\n\t\tafraid\nai\n\taim\n\tair\n",
        );
        let abide = english(&cards, "abide");
        assert_eq!(abide.section, "ab");
        assert_eq!(abide.meaning, "字首 ab · abi");
        assert!(cards.iter().any(|card| card.english == "ability"));
        assert_eq!(english(&cards, "afraid").meaning, "字首 ab · afr");
        assert_eq!(english(&cards, "aim").section, "ai");
        assert!(cards.iter().all(|card| card.english != "abi"));
    }

    #[test]
    fn decodes_utf8_bom_and_big5() {
        assert_eq!(decode_bytes(&[0xEF, 0xBB, 0xBF, b'a', b'b']), "ab");
        assert_eq!(decode_bytes(&[0xA4, 0xA4]), "中");
    }

    #[test]
    fn parses_real_folder_when_present() {
        let dir = Path::new(r"D:\eng");
        if !dir.exists() {
            return;
        }
        let decks = load_decks(dir).expect("read D:\\eng");
        let daily = decks
            .iter()
            .find(|deck| deck.name == "daily.txt")
            .expect("daily");
        let impressive = english(&daily.cards, "impressive");
        assert_eq!(impressive.syllables, "im pre ssive");
        assert_eq!(impressive.section, "情緒");
        let numerator = english(&daily.cards, "numerator");
        assert_eq!(numerator.syllables, "numera tor");
        assert_eq!(numerator.section, "數學");
        assert_eq!(english(&daily.cards, "square bracket").meaning, "中括弧");

        let phrase = decks
            .iter()
            .find(|deck| deck.name == "phrase.txt")
            .expect("phrase");
        assert!(phrase.cards.iter().any(|card| card.english == "stick to"));

        let prefix = decks
            .iter()
            .find(|deck| deck.name == "prefix.txt")
            .expect("prefix");
        assert_eq!(english(&prefix.cards, "abide").meaning, "字首 ab · abi");

        let same = decks
            .iter()
            .find(|deck| deck.name == "相同.txt")
            .expect("相同");
        assert!(english(&same.cards, "somehow")
            .meaning
            .contains("不知怎麼地"));

        let total: usize = decks.iter().map(|deck| deck.cards.len()).sum();
        assert!(total > 100, "only parsed {total} cards");
    }
}
