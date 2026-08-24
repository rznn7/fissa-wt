const SEPARATORS: [char; 5] = ['-', '/', '_', '.', ' '];

/// A single-line editable value with a cursor, measured in characters.
pub struct TextInput {
    value: String,
    cursor: usize,
}

impl TextInput {
    pub fn new(value: String) -> Self {
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn len(&self) -> usize {
        self.value.chars().count()
    }

    fn byte_of(&self, index: usize) -> usize {
        self.value
            .char_indices()
            .nth(index)
            .map(|(byte, _)| byte)
            .unwrap_or(self.value.len())
    }

    fn char_before_cursor(&self) -> Option<char> {
        self.value.chars().nth(self.cursor.checked_sub(1)?)
    }

    /// A typed space becomes a dash, and is dropped where that would double one up.
    pub fn insert(&mut self, ch: char) {
        if ch == ' ' {
            if self.char_before_cursor() == Some('-') {
                return;
            }
            return self.insert_raw('-');
        }
        self.insert_raw(ch);
    }

    fn insert_raw(&mut self, ch: char) {
        let at = self.byte_of(self.cursor);
        self.value.insert(at, ch);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        self.value.remove(self.byte_of(self.cursor));
    }

    pub fn delete(&mut self) {
        if self.cursor < self.len() {
            self.value.remove(self.byte_of(self.cursor));
        }
    }

    pub fn delete_word_back(&mut self) {
        while self
            .char_before_cursor()
            .is_some_and(|ch| SEPARATORS.contains(&ch))
        {
            self.backspace();
        }
        while self
            .char_before_cursor()
            .is_some_and(|ch| !SEPARATORS.contains(&ch))
        {
            self.backspace();
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(value: &str) -> TextInput {
        TextInput::new(String::from(value))
    }

    fn typed(text: &str) -> TextInput {
        let mut input = input("");
        for ch in text.chars() {
            input.insert(ch);
        }
        input
    }

    #[test]
    fn test_new_parks_the_cursor_after_the_value() {
        let input = input("spe-1");
        assert_eq!(input.cursor(), 5);
    }

    #[test]
    fn test_insert_lands_at_the_cursor_not_the_end() {
        let mut input = input("spe-1");
        input.home();
        input.insert('x');
        assert_eq!(input.value(), "xspe-1");
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn test_a_typed_space_becomes_a_dash() {
        assert_eq!(typed("spe 11667").value(), "spe-11667");
    }

    #[test]
    fn test_a_run_of_spaces_collapses_to_one_dash() {
        assert_eq!(typed("foo   bar").value(), "foo-bar");
    }

    #[test]
    fn test_a_space_after_a_typed_dash_is_dropped() {
        assert_eq!(typed("foo- bar").value(), "foo-bar");
    }

    #[test]
    fn test_typed_dashes_are_never_collapsed() {
        assert_eq!(typed("foo--bar").value(), "foo--bar");
    }

    #[test]
    fn test_a_leading_space_still_opens_with_a_dash() {
        assert_eq!(typed(" foo").value(), "-foo");
    }

    #[test]
    fn test_backspace_removes_the_char_before_the_cursor() {
        let mut input = input("spe-1");
        input.left();
        input.backspace();
        assert_eq!(input.value(), "spe1");
        assert_eq!(input.cursor(), 3);
    }

    #[test]
    fn test_backspace_at_the_start_is_a_no_op() {
        let mut input = input("abc");
        input.home();
        input.backspace();
        assert_eq!(input.value(), "abc");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn test_delete_removes_the_char_under_the_cursor() {
        let mut input = input("abc");
        input.home();
        input.delete();
        assert_eq!(input.value(), "bc");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn test_delete_at_the_end_is_a_no_op() {
        let mut input = input("abc");
        input.delete();
        assert_eq!(input.value(), "abc");
    }

    #[test]
    fn test_left_stops_at_the_start_and_right_stops_at_the_end() {
        let mut input = input("ab");
        for _ in 0..5 {
            input.left();
        }
        assert_eq!(input.cursor(), 0);
        for _ in 0..5 {
            input.right();
        }
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn test_home_and_end_jump_to_the_edges() {
        let mut input = input("abc");
        input.home();
        assert_eq!(input.cursor(), 0);
        input.end();
        assert_eq!(input.cursor(), 3);
    }

    #[test]
    fn test_delete_word_back_drops_the_last_segment() {
        let mut input = input("feature/spe-11667");
        input.delete_word_back();
        assert_eq!(input.value(), "feature/spe-");
    }

    #[test]
    fn test_delete_word_back_eats_trailing_separators_first() {
        let mut input = input("feature/spe/");
        input.delete_word_back();
        assert_eq!(input.value(), "feature/");
    }

    #[test]
    fn test_delete_word_back_on_an_empty_value_is_a_no_op() {
        let mut input = input("");
        input.delete_word_back();
        assert_eq!(input.value(), "");
    }

    #[test]
    fn test_editing_counts_characters_not_bytes() {
        let mut input = input("héllo");
        input.home();
        input.right();
        input.delete();
        assert_eq!(input.value(), "hllo");
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn test_inserting_after_a_multibyte_char_does_not_split_it() {
        let mut input = input("é");
        input.insert('x');
        assert_eq!(input.value(), "éx");
        assert_eq!(input.cursor(), 2);
    }
}
