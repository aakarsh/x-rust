// src/main.rs

use ncurses as nc;
use simplelog::{Config, LevelFilter, WriteLogger};
use std::collections::HashMap;
use std::env;
// FIX 1: Removed unused `OpenOptions`
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

// --- Core Data Structures (Gap Buffer, Lines, Buffer) ---
// ... (This section is unchanged, so it is omitted for brevity) ...
// (You can copy the structs GapLine, XLine, Buf, BufList from the previous answer)
/// A gap buffer implementation for a single line of text.
struct GapLine {
    buf: Vec<char>,
    gap_start: usize,
    gap_end: usize,
}

impl GapLine {
    const DEFAULT_GAP_SIZE: usize = 16;

    pub fn new(initial_capacity: usize) -> Self {
        let capacity = if initial_capacity == 0 {
            Self::DEFAULT_GAP_SIZE
        } else {
            initial_capacity
        };
        Self {
            buf: vec!['\0'; capacity],
            gap_start: 0,
            gap_end: capacity,
        }
    }

    pub fn from_str(s: &str) -> Self {
        let mut gl = GapLine::new(s.len() + Self::DEFAULT_GAP_SIZE);
        for ch in s.chars() {
            gl.insert_char(ch);
        }
        gl
    }

    fn expand(&mut self) {
        let old_size = self.buf.len();
        let new_size = if old_size == 0 {
            Self::DEFAULT_GAP_SIZE
        } else {
            old_size * 2
        };

        let mut new_buf = vec!['\0'; new_size];
        let old_tail_len = old_size - self.gap_end;

        new_buf[..self.gap_start].copy_from_slice(&self.buf[..self.gap_start]);
        let new_tail_start = new_size - old_tail_len;
        new_buf[new_tail_start..].copy_from_slice(&self.buf[self.gap_end..]);

        self.buf = new_buf;
        self.gap_end = new_tail_start;
    }

    pub fn insert_char(&mut self, ch: char) {
        if self.gap_start + 1 >= self.gap_end {
            self.expand();
        }
        self.buf[self.gap_start] = ch;
        self.gap_start += 1;
    }

    pub fn to_string(&self) -> String {
        let mut s = String::with_capacity(self.buf.len());
        s.extend(&self.buf[..self.gap_start]);
        s.extend(&self.buf[self.gap_end..]);
        s
    }

    pub fn gap_info(&self) -> String {
        format!(
            "[gap_start: {}, gap_end: {}, size: {}]",
            self.gap_start,
            self.gap_end,
            self.buf.len()
        )
    }

    pub fn len(&self) -> usize {
        self.gap_start + (self.buf.len() - self.gap_end)
    }
}
struct XLine {
    #[allow(dead_code)]
    line_number: usize,
    data: String,
    gap_data: GapLine,
}
impl XLine {
    fn new(line_number: usize, data: String) -> Self {
        let gap_data = GapLine::from_str(&data);
        Self {
            line_number,
            data,
            gap_data,
        }
    }
    fn size(&self) -> usize {
        self.data.chars().count()
    }
}
struct Buf {
    file_path: PathBuf,
    buffer_name: String,
    lines: Vec<XLine>,
    modified: bool,
}
impl Buf {
    fn from_path(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut lines = Vec::new();
        for (i, line_result) in reader.lines().enumerate() {
            let line_str = line_result?;
            log::info!("{}", &line_str);
            lines.push(XLine::new(i, line_str));
        }
        Ok(Self {
            file_path: path.to_path_buf(),
            buffer_name: path.to_string_lossy().into_owned(),
            lines,
            modified: false,
        })
    }
}
struct BufList {
    buffers: Vec<Buf>,
    current_idx: usize,
}
impl BufList {
    fn new(first_buffer: Buf) -> Self {
        Self {
            buffers: vec![first_buffer],
            current_idx: 0,
        }
    }
    fn append(&mut self, buffer: Buf) {
        self.buffers.push(buffer);
        self.current_idx = self.buffers.len() - 1;
    }
    fn get_current_buffer(&self) -> &Buf {
        &self.buffers[self.current_idx]
    }
    #[allow(dead_code)]
    fn get_current_buffer_mut(&mut self) -> &mut Buf {
        &mut self.buffers[self.current_idx]
    }
}
// --- UI (Display Window) ---
// ... (This section is also unchanged) ...
struct DisplayWindow {
    window: nc::WINDOW,
}
impl DisplayWindow {
    fn new(nlines: i32, ncols: i32, begin_y: i32, begin_x: i32) -> Self {
        let window = nc::newwin(nlines, ncols, begin_y, begin_x);
        Self { window }
    }
    fn get_height(&self) -> i32 {
        nc::getmaxy(self.window)
    }
    fn get_width(&self) -> i32 {
        nc::getmaxx(self.window)
    }
    fn refresh(&self) {
        nc::wrefresh(self.window);
    }
    fn move_cursor(&self, y: i32, x: i32) {
        nc::wmove(self.window, y, x);
    }
    fn display_line(&self, y: i32, x: i32, text: &str) {
        self.move_cursor(y, x);
        nc::wprintw(self.window, text);
    }
    fn display_str(&self, text: &str) {
        nc::wprintw(self.window, text);
    }
    fn clear(&self) {
        nc::wclear(self.window);
        self.move_cursor(0, 0);
    }
    fn read_input(&self, prompt: &str) -> String {
        self.clear();
        self.display_line(0, 0, prompt);
        self.refresh();
        let mut input = String::new();
        nc::echo();
        nc::wgetstr(self.window, &mut input);
        nc::noecho();
        self.clear();
        input
    }
}
impl Drop for DisplayWindow {
    fn drop(&mut self) {
        nc::delwin(self.window);
    }
}


// --- Editor Logic (Modes, Commands, Editor) ---

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorMode {
    Command,
    Insert,
    Search,
}

// FIX 2: A new, idiomatic way to make trait objects cloneable.
trait EditorCommand {
    fn execute(&self, editor: &mut Editor) -> EditorMode;
    // Every command must now know how to clone itself into a Box.
    fn clone_dyn(&self) -> Box<dyn EditorCommand>;
}

// We can now implement Clone for the Box itself.
impl Clone for Box<dyn EditorCommand> {
    fn clone(&self) -> Self {
        self.clone_dyn()
    }
}

struct Mode {
    name: String,
    keymap: HashMap<String, Box<dyn EditorCommand>>,
}

impl Mode {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            keymap: HashMap::new(),
        }
    }

    fn add_command(&mut self, keys: &[&str], command: Box<dyn EditorCommand>) {
        for key in keys {
            // Now we can just use .clone() on the box.
            self.keymap.insert(key.to_string(), command.clone());
        }
    }

    // FIX 3: Changed return type to make borrow checker happy later.
    fn lookup(&self, cmd: &str) -> Option<&Box<dyn EditorCommand>> {
        self.keymap.get(cmd)
    }
}

// --- Command Implementations ---

#[derive(Clone)]
struct Quit;
impl EditorCommand for Quit {
    fn execute(&self, editor: &mut Editor) -> EditorMode {
        editor.quit = true;
        editor.mode
    }
    fn clone_dyn(&self) -> Box<dyn EditorCommand> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
struct MovePoint {
    dy: i32,
    dx: i32,
}
impl EditorCommand for MovePoint {
    fn execute(&self, editor: &mut Editor) -> EditorMode {
        editor.move_point(self.dy, self.dx);
        editor.mode
    }
    fn clone_dyn(&self) -> Box<dyn EditorCommand> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
struct MoveToLineEdge {
    to_end: bool,
}
impl EditorCommand for MoveToLineEdge {
    fn execute(&self, editor: &mut Editor) -> EditorMode {
        editor.move_to_line_edge(self.to_end);
        editor.mode
    }
    fn clone_dyn(&self) -> Box<dyn EditorCommand> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
struct MoveToFileEdge {
    to_end: bool,
}
impl EditorCommand for MoveToFileEdge {
    fn execute(&self, editor: &mut Editor) -> EditorMode {
        editor.move_to_file_edge(self.to_end);
        editor.mode
    }
    fn clone_dyn(&self) -> Box<dyn EditorCommand> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
struct MovePage {
    increment: i32,
}
impl EditorCommand for MovePage {
    fn execute(&self, editor: &mut Editor) -> EditorMode {
        editor.move_page(self.increment);
        editor.mode
    }
    fn clone_dyn(&self) -> Box<dyn EditorCommand> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
struct ToggleLineNumbers;
impl EditorCommand for ToggleLineNumbers {
    fn execute(&self, editor: &mut Editor) -> EditorMode {
        editor.line_number_show = !editor.line_number_show;
        editor.mark_redisplay();
        editor.mode
    }
    fn clone_dyn(&self) -> Box<dyn EditorCommand> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
struct OpenFile;
impl EditorCommand for OpenFile {
    fn execute(&self, editor: &mut Editor) -> EditorMode {
        let file_path_str = editor.mode_read_input("File: ");
        let file_path = PathBuf::from(file_path_str);
        match Buf::from_path(&file_path) {
            Ok(new_buf) => {
                editor.buffers.append(new_buf);
                editor.mark_redisplay();
            }
            Err(e) => {
                let err_msg = format!("Error opening file: {}", e);
                editor.mode_window.display_line(0, 0, &err_msg);
                nc::wgetch(editor.mode_window.window); // wait for keypress
            }
        }
        editor.mark_redisplay();
        EditorMode::Command
    }
    fn clone_dyn(&self) -> Box<dyn EditorCommand> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
struct Search;
impl EditorCommand for Search {
    fn execute(&self, editor: &mut Editor) -> EditorMode {
        let _search_string = editor.mode_read_input("Search Forward: ");
        // TODO: Implement actual search logic
        editor.mark_redisplay();
        EditorMode::Search
    }
    fn clone_dyn(&self) -> Box<dyn EditorCommand> {
        Box::new(self.clone())
    }
}

struct Editor {
    modes: Vec<Mode>,
    mode: EditorMode,
    screen_height: i32,
    screen_width: i32,
    mode_window: DisplayWindow,
    buffer_window: DisplayWindow,
    buffers: BufList,
    redisplay: bool,
    quit: bool,
    cursor: (i32, i32),
    start_line: usize,
    line_number_show: bool,
}

impl Editor {
    const MODE_PADDING: i32 = 1;

    fn new(initial_buffer: Buf) -> Self {
        nc::initscr();
        nc::raw();
        nc::noecho();
        nc::keypad(nc::stdscr(), true);

        let mut screen_height = 0;
        let mut screen_width = 0;
        nc::getmaxyx(nc::stdscr(), &mut screen_height, &mut screen_width);

        let buffer_window =
            DisplayWindow::new(screen_height - Self::MODE_PADDING, screen_width, 0, 0);
        let mode_window = DisplayWindow::new(
            Self::MODE_PADDING,
            screen_width,
            screen_height - Self::MODE_PADDING,
            0,
        );

        let mut cmd_mode = Mode::new("CMD");
        cmd_mode.add_command(&["q"], Box::new(Quit));
        cmd_mode.add_command(&["j", "KEY_DOWN"], Box::new(MovePoint { dy: 1, dx: 0 }));
        cmd_mode.add_command(&["k", "KEY_UP"], Box::new(MovePoint { dy: -1, dx: 0 }));
        cmd_mode.add_command(&["l", "KEY_RIGHT"], Box::new(MovePoint { dy: 0, dx: 1 }));
        cmd_mode.add_command(&["h", "KEY_LEFT"], Box::new(MovePoint { dy: 0, dx: -1 }));
        cmd_mode.add_command(&["^", "0", "KEY_HOME"], Box::new(MoveToLineEdge { to_end: false }));
        cmd_mode.add_command(&["$", "KEY_END"], Box::new(MoveToLineEdge { to_end: true }));
        cmd_mode.add_command(&["G"], Box::new(MoveToFileEdge { to_end: true }));
        cmd_mode.add_command(&[" ", "KEY_NPAGE"], Box::new(MovePage { increment: 1 }));
        cmd_mode.add_command(&["KEY_PPAGE"], Box::new(MovePage { increment: -1 }));
        cmd_mode.add_command(&["."], Box::new(ToggleLineNumbers));
        cmd_mode.add_command(&["o"], Box::new(OpenFile));
        cmd_mode.add_command(&["/"], Box::new(Search));

        let insert_mode = Mode::new("INSERT");
        let search_mode = Mode::new("SEARCH");

        Self {
            modes: vec![cmd_mode, insert_mode, search_mode],
            mode: EditorMode::Command,
            screen_height,
            screen_width,
            mode_window,
            buffer_window,
            buffers: BufList::new(initial_buffer),
            redisplay: true,
            quit: false,
            cursor: (0, 0),
            start_line: 0,
            line_number_show: false,
        }
    }

    fn run(&mut self) {
        while !self.quit {
            if self.redisplay {
                self.display_buffer();
                self.redisplay = false;
            }
            self.display_mode_line();
            self.display_cursor();

            let cmd_str = self.parse_cmd();
            self.run_cmd(&cmd_str);
        }
    }

    fn run_cmd(&mut self, cmd: &str) {
        // FIX 4: Clone the command after lookup to satisfy the borrow checker.
        // `.cloned()` works because we implemented Clone for `Box<dyn EditorCommand>`.
        if let Some(command) = self.modes[self.mode as usize].lookup(cmd).cloned() {
            let next_mode = command.execute(self);
            if self.mode != next_mode {
                self.mode = next_mode;
                self.mark_redisplay();
            }
        }
    }
    
    // ... (rest of Editor impl is unchanged) ...
    fn parse_cmd(&self) -> String {
        let ch = nc::getch();
        nc::keyname(ch)
            .map(|s| s.to_string())
            .unwrap_or_else(|| (ch as u8 as char).to_string())
    }

    fn display_mode_line(&self) {
        let buffer = self.buffers.get_current_buffer();
        let modified_char = if buffer.modified { "*" } else { "-" };
        let mode_name = &self.modes[self.mode as usize].name;
        
        let mode_line = format!(
            "[{}] {} ------ [{}]",
            modified_char,
            buffer.buffer_name,
            mode_name
        );
        
        self.mode_window.clear();
        self.mode_window.display_line(0, 0, &mode_line);
        self.mode_window.refresh();
    }

    fn display_buffer(&self) {
        self.buffer_window.clear();
        let buffer = self.buffers.get_current_buffer();
        let window_height = self.buffer_window.get_height() as usize;

        for (i, line_idx) in (self.start_line..buffer.lines.len()).enumerate() {
            if i >= window_height {
                break;
            }
            
            let line = &buffer.lines[line_idx];
            let mut display_text = String::new();

            if self.line_number_show {
                display_text.push_str(&format!(
                    "{:5}: {} {}", 
                    line_idx + 1, 
                    line.data, 
                    line.gap_data.gap_info()
                ));
            } else {
                display_text.push_str(&line.data);
            }

            self.buffer_window.display_line(i as i32, 0, &display_text);
        }
        self.buffer_window.refresh();
    }
    fn display_cursor(&self) {
        nc::mv(self.cursor.0, self.cursor.1);
        nc::refresh();
    }

    fn mark_redisplay(&mut self) {
        self.redisplay = true;
    }
    // FIX 5: Changed to `&mut self` because it calls `mark_redisplay`.
    fn mode_read_input(&mut self, prompt: &str) -> String {
        let input = self.mode_window.read_input(prompt);
        self.mark_redisplay(); // Reading input clears the screen, so we must redraw
        input
    }
    fn get_current_line_idx(&self) -> usize {
        self.start_line + self.cursor.0 as usize
    }

    fn get_current_line_len(&self) -> usize {
        self.buffers.get_current_buffer().lines
            .get(self.get_current_line_idx())
            .map_or(0, |l| l.size())
    }

    fn move_point(&mut self, dy: i32, dx: i32) {
        let buffer = self.buffers.get_current_buffer();
        let num_lines = buffer.lines.len();
        let window_height = self.buffer_window.get_height();
        
        let mut new_y = self.cursor.0 + dy;
        new_y = new_y.max(0).min(window_height - 1);
        
        let potential_line_idx = self.start_line + new_y as usize;
        if potential_line_idx >= num_lines {
            new_y = (num_lines.saturating_sub(1).saturating_sub(self.start_line)) as i32;
        }
        self.cursor.0 = new_y.max(0);

        let mut new_x = self.cursor.1 + dx;
        let line_len = self.get_current_line_len();
        new_x = new_x.max(0).min(line_len as i32);
        self.cursor.1 = new_x;
    }

    fn move_to_line_edge(&mut self, to_end: bool) {
        if to_end {
            self.cursor.1 = self.get_current_line_len() as i32;
        } else {
            self.cursor.1 = 0;
        }
    }

    fn move_to_file_edge(&mut self, to_end: bool) {
        if to_end {
            let num_lines = self.buffers.get_current_buffer().lines.len();
            let window_height = self.buffer_window.get_height() as usize;
            self.start_line = num_lines.saturating_sub(window_height).max(0);
            self.cursor.0 = (num_lines.saturating_sub(1).saturating_sub(self.start_line)) as i32;
        } else {
            self.start_line = 0;
            self.cursor.0 = 0;
        }
        self.mark_redisplay();
    }
    
    fn move_page(&mut self, increment: i32) {
        let num_lines = self.buffers.get_current_buffer().lines.len();
        let page_size = self.buffer_window.get_height() as usize;

        let new_start_line = if increment > 0 {
            (self.start_line + page_size).min(num_lines.saturating_sub(1))
        } else {
            self.start_line.saturating_sub(page_size)
        };
        
        self.start_line = new_start_line;
        self.mark_redisplay();
    }
}


impl Drop for Editor {
    fn drop(&mut self) {
        nc::endwin();
    }
}

// --- Main Application ---
// ... (This section is unchanged) ...
fn main() {
    WriteLogger::init(
        LevelFilter::Info,
        Config::default(),
        File::create("x-debug.log").unwrap(),
    )
    .expect("Failed to initialize logger");

    log::info!("x: started");

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <filename>", args[0]);
        return;
    }

    let file_path = PathBuf::from(&args[1]);
    let initial_buffer = match Buf::from_path(&file_path) {
        Ok(buf) => buf,
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                 Buf {
                    file_path: file_path.clone(),
                    buffer_name: file_path.to_string_lossy().into_owned(),
                    lines: vec![XLine::new(0, String::new())],
                    modified: false,
                }
            } else {
                eprintln!("Error loading file '{}': {}", args[1], e);
                return;
            }
        }
    };
    
    let mut editor = Editor::new(initial_buffer);
    editor.run();

    log::info!("x: ended");
}
