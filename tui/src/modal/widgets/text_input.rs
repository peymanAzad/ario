use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Default)]
pub struct TextInput {
    pub buffer: String,
    pub editing: bool,
}

impl TextInput {
    pub fn start(&mut self, initial: String) {
        self.buffer = initial;
        self.editing = true;
    }

    pub fn input(&mut self, c: char) {
        if self.editing && !c.is_control() {
            self.buffer.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.editing
            && let Some((index, _)) = self.buffer.grapheme_indices(true).next_back()
        {
            self.buffer.truncate(index);
        }
    }

    pub fn take(&mut self) -> String {
        self.editing = false;
        std::mem::take(&mut self.buffer)
    }

    pub fn cancel(&mut self) {
        self.editing = false;
        self.buffer.clear();
    }
}
