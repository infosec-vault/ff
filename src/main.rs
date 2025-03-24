use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
  };
  use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
  };
  use serde_json::Value;
  use std::collections::HashMap;
  use std::env;
  use std::fs::{self, File, OpenOptions};
  use std::io::{self, BufRead, BufReader, Read, Write};
  use std::path::{Path, PathBuf};
  use std::process::{Command, Stdio};
  use std::time::{SystemTime, UNIX_EPOCH};
  
  struct App {
    entries: Vec<(PathBuf, u64, bool)>, 
    current_dir: PathBuf,
    selected_index: usize,
    list_state: ListState,
    should_quit: bool,
    show_help: bool,
    search_mode: bool,
    search_query: String,
    bind_mode: bool,
    bind_command: String,
    show_files: bool,
    unfiltered_entries: Vec<(PathBuf, u64, bool)>,
    help_scroll_state: ListState,
    help_scroll_index: usize,
  }
  
  impl App {
    fn new(current_dir: PathBuf, entries: Vec<(PathBuf, u64, bool)>) -> Self {
      let mut list_state = ListState::default();
      list_state.select(Some(0));
      
      let mut help_scroll_state = ListState::default();
      help_scroll_state.select(Some(0));
      
      App {
        entries,
        unfiltered_entries: Vec::new(),
        current_dir,
        selected_index: 0,
        list_state,
        should_quit: false,
        show_help: false,
        search_mode: false,
        search_query: String::new(),
        bind_mode: false,
        bind_command: String::new(),
        show_files: false,
        help_scroll_state,
        help_scroll_index: 0,
      }
    }
  
    fn next(&mut self) {
      if !self.entries.is_empty() {
        self.selected_index = (self.selected_index + 1) % self.entries.len();
        self.list_state.select(Some(self.selected_index));
      }
    }
  
    fn previous(&mut self) {
      if !self.entries.is_empty() {
        self.selected_index = if self.selected_index > 0 {
          self.selected_index - 1
        } else {
          self.entries.len() - 1
        };
        self.list_state.select(Some(self.selected_index));
      }
    }
    
    fn help_next(&mut self) {
      self.help_scroll_index = self.help_scroll_index + 1;
      self.help_scroll_state.select(Some(self.help_scroll_index));
    }
    
    fn help_previous(&mut self) {
      if self.help_scroll_index > 0 {
        self.help_scroll_index -= 1;
        self.help_scroll_state.select(Some(self.help_scroll_index));
      }
    }
  
    fn toggle_help(&mut self) {
      self.show_help = !self.show_help;
    }
  
    fn toggle_files_dirs(&mut self) {
      self.show_files = !self.show_files;
    }
  
    fn start_search(&mut self) {
      if !self.search_mode && !self.bind_mode {
        self.search_mode = true;
        self.search_query = String::new();
        self.unfiltered_entries = self.entries.clone();
      }
    }
  
    fn start_bind(&mut self, current_command: String) {
      if !self.search_mode && !self.bind_mode {
        self.bind_mode = true;
        self.bind_command = current_command;
      } else if self.bind_mode {
        self.bind_mode = false;
      }
    }
  
    fn end_search(&mut self) {
      if self.search_mode {
        self.search_mode = false;
        self.search_query = String::new();
        self.entries = self.unfiltered_entries.clone();
        self.unfiltered_entries = Vec::new();
        if !self.entries.is_empty() {
          self.selected_index = 0;
          self.list_state.select(Some(0));
        }
      }
    }
  
    fn end_bind(&mut self) {
      if self.bind_mode {
        self.bind_mode = false;
        self.bind_command = String::new();
      }
    }
  
    fn update_search(&mut self, character: char) {
      self.search_query.push(character);
      self.filter_entries();
    }
  
    fn update_bind(&mut self, character: char) {
      self.bind_command.push(character);
    }
  
    fn backspace_search(&mut self) {
      if !self.search_query.is_empty() {
        self.search_query.pop();
        self.filter_entries();
      }
    }
  
    fn backspace_bind(&mut self) {
      if !self.bind_command.is_empty() {
        self.bind_command.pop();
      }
    }
  
    fn filter_entries(&mut self) {
      let query = self.search_query.to_lowercase();
      self.entries = self.unfiltered_entries
        .iter()
        .filter(|(path, _, _)| {
          if let Some(name) = path.file_name() {
            name.to_string_lossy().to_lowercase().contains(&query)
          } else {
            false
          }
        })
        .cloned()
        .collect();
  
      if !self.entries.is_empty() {
        self.selected_index = 0;
        self.list_state.select(Some(0));
      }
    }
  }
  
  fn main() -> io::Result<()> {
    if let Err(e) = run_app() {
      eprintln!("Error: {}", e);
    }
    Ok(())
  }
  
  fn run_app() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
  
    let history_file = get_history_file_path()?;
    let history = read_history(&history_file)?;
    let current_dir = env::current_dir()?;
    let entries = get_sorted_entries(&current_dir, &history, false)?;
    let mut app = App::new(current_dir, entries);
  
    let res = run_ui(&mut terminal, &mut app, &history_file);
  
    disable_raw_mode()?;
    execute!(
      terminal.backend_mut(),
      LeaveAlternateScreen,
      DisableMouseCapture
    )?;
    terminal.show_cursor()?;
  
    if let Some(selected_dir) = res? {
      update_history(&history_file, &selected_dir)?;
  
      let shell = env::var("SHELL").unwrap_or_else(|_| String::from("/bin/bash"));
      
      let custom_command = get_custom_script(&selected_dir)?;
      
      let mut shell_command = format!("cd '{}'", selected_dir.display());
      
      if let Some(cmd) = custom_command {
        shell_command.push_str(&format!(" && {}", cmd));
      }
      
      shell_command.push_str(&format!(" && exec {}", shell));
      
      let status = Command::new(&shell)
        .arg("-c")
        .arg(shell_command)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
  
      if !status.success() {
        eprintln!("Failed to change directory.");
      }
    }
  
    Ok(())
  }
  
  fn run_ui<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    history_file: &Path,
  ) -> io::Result<Option<PathBuf>> {
    loop {
      terminal.draw(|f| ui(f, app))?;
  
      if let Event::Key(key) = event::read()? {
        if app.search_mode {
          match key.code {
            KeyCode::Esc => {
              app.end_search();
            }
            KeyCode::Backspace => {
              app.backspace_search();
            }
            KeyCode::Enter => {
              if !app.entries.is_empty() {
                let selected_path = app.entries[app.selected_index].0.clone();
                let is_dir = app.entries[app.selected_index].2;
                
                app.end_search();
                
                if is_dir {
                  app.current_dir = selected_path;
                  let entries = get_sorted_entries(&app.current_dir, &read_history(history_file)?, app.show_files)?;
                  app.entries = entries;
                  app.selected_index = 0;
                  app.list_state.select(Some(0));
                } else {
                  if let Some(parent) = selected_path.parent() {
                    app.current_dir = parent.to_path_buf();
                    let entries = get_sorted_entries(&app.current_dir, &read_history(history_file)?, app.show_files)?;
                    app.entries = entries;
                    app.selected_index = 0;
                    app.list_state.select(Some(0));
                  }
                }
              }
            }
            KeyCode::Char(' ') => {
              app.end_search();
            }
            KeyCode::Down | KeyCode::Char('j') => {
              app.next();
            }
            KeyCode::Up | KeyCode::Char('k') => {
              app.previous();
            }
            KeyCode::Char(c) => {
              app.update_search(c);
            }
            _ => {}
          }
        } else if app.bind_mode {
          match key.code {
            KeyCode::Esc => {
              app.end_bind();
            }
            KeyCode::Backspace => {
              app.backspace_bind();
            }
            KeyCode::Enter => {
              save_custom_script(&app.current_dir, &app.bind_command)?;
              app.end_bind();
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
              app.end_bind();
            }
            KeyCode::Char(c) => {
              app.update_bind(c);
            }
            _ => {}
          }
        } else if app.show_help {
          match key.code {
            KeyCode::Esc => {
              app.show_help = false;
            }
            KeyCode::Down | KeyCode::Char('j') => {
              app.help_next();
            }
            KeyCode::Up | KeyCode::Char('k') => {
              app.help_previous();
            }
            KeyCode::Char('h') => {
              app.toggle_help();
            }
            KeyCode::Char('q') => {
              app.show_help = false;
            }
            _ => {}
          }
        } else {
          match key.code {
            KeyCode::Char('q') => {
              return Ok(Some(app.current_dir.clone()));
            }
            KeyCode::Char('h') => {
              app.toggle_help();
              app.help_scroll_index = 0;
              app.help_scroll_state.select(Some(0));
            }
            KeyCode::Char('f') => {
              app.toggle_files_dirs();
              let entries = get_sorted_entries(&app.current_dir, &read_history(history_file)?, app.show_files)?;
              app.entries = entries;
              app.selected_index = 0;
              app.list_state.select(Some(0));
            }
            KeyCode::Char(' ') => {
              app.start_search();
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
              let current_command = get_custom_script(&app.current_dir)?.unwrap_or_default();
              app.start_bind(current_command);
            }
            KeyCode::Down | KeyCode::Char('j') => {
              app.next();
            }
            KeyCode::Up | KeyCode::Char('k') => {
              app.previous();
            }
            KeyCode::Backspace => {
              if let Some(parent) = app.current_dir.parent() {
                let new_dir = parent.to_path_buf();
                let entries = get_sorted_entries(&new_dir, &read_history(history_file)?, app.show_files)?;
                app.current_dir = new_dir;
                app.entries = entries;
                app.selected_index = 0;
                app.list_state.select(Some(0));
              }
            }
            KeyCode::Enter => {
              if !app.entries.is_empty() {
                let selected_path = app.entries[app.selected_index].0.clone();
                let is_dir = app.entries[app.selected_index].2;
                
                if is_dir {
                  app.current_dir = selected_path;
                  let entries = get_sorted_entries(&app.current_dir, &read_history(history_file)?, app.show_files)?;
                  app.entries = entries;
                  app.selected_index = 0;
                  app.list_state.select(Some(0));
                } else {
                  if let Some(parent) = selected_path.parent() {
                    app.current_dir = parent.to_path_buf();
                    return Ok(Some(app.current_dir.clone()));
                  }
                }
              }
            }
            KeyCode::Esc => {
              if app.show_help {
                app.show_help = false;
              } else {
                app.should_quit = true;
                return Ok(None);
              }
            }
            _ => {}
          }
        }
      }
    }
  }
  
  fn ui(f: &mut Frame, app: &mut App) {
    let chunks = if app.search_mode || app.bind_mode {
      Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
          [
            Constraint::Min(1),
            Constraint::Length(3),
          ]
          .as_ref(),
        )
        .split(f.area())
    } else {
      Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(1)].as_ref())
        .split(f.area())
    };
    
    let current_dir_str = app.current_dir.display().to_string();
    
    if app.show_help {
      let help_text = vec![
        ListItem::new(Line::from(Span::styled("Keyboard Controls:", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)))),
        ListItem::new(Line::from("")),
        ListItem::new(Line::from(vec![
          Span::styled("↑/k", Style::default().fg(Color::Yellow)),
          Span::raw(" - Move selection up"),
        ])),
        ListItem::new(Line::from(vec![
          Span::styled("↓/j", Style::default().fg(Color::Yellow)),
          Span::raw(" - Move selection down"),
        ])),
        ListItem::new(Line::from(vec![
          Span::styled("Backspace", Style::default().fg(Color::Yellow)),
          Span::raw(" - Go to parent directory"),
        ])),
        ListItem::new(Line::from(vec![
          Span::styled("Space", Style::default().fg(Color::Yellow)),
          Span::raw(" - Start/stop search"),
        ])),
        ListItem::new(Line::from(vec![
          Span::styled("Ctrl+b", Style::default().fg(Color::Yellow)),
          Span::raw(" - Bind/edit command for current directory"),
        ])),
        ListItem::new(Line::from(vec![
          Span::styled("f", Style::default().fg(Color::Yellow)),
          Span::raw(" - Toggle files/directories view"),
        ])),
        ListItem::new(Line::from(vec![
          Span::styled("h", Style::default().fg(Color::Yellow)),
          Span::raw(" - Toggle help"),
        ])),
        ListItem::new(Line::from(vec![
          Span::styled("q", Style::default().fg(Color::Yellow)),
          Span::raw(" - Quit and cd into current directory"),
        ])),
        ListItem::new(Line::from(vec![
          Span::styled("Esc", Style::default().fg(Color::Yellow)),
          Span::raw(" - Close help / Quit"),
        ])),
      ];
  
      let help = List::new(help_text)
        .block(Block::default().borders(Borders::ALL).title(format!("Help ({})", current_dir_str)))
        .highlight_style(
          Style::default()
            .fg(Color::Black)
            .bg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
        );
      
      f.render_stateful_widget(help, chunks[0], &mut app.help_scroll_state);
    } else {
      let items: Vec<ListItem> = app
        .entries
        .iter()
        .map(|(path, _, is_dir)| {
          let name = path.file_name().unwrap_or_default().to_string_lossy();
          let display_text = if *is_dir {
            format!("{}/", name)
          } else {
            name.to_string()
          };
          
          let style = if *is_dir {
            Style::default().fg(Color::Blue)
          } else {
            Style::default().fg(Color::White)
          };
          
          ListItem::new(Line::from(vec![Span::styled(display_text, style)]))
        })
        .collect();
  
      let dirs_list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(current_dir_str))
        .highlight_style(
          Style::default()
            .fg(Color::Black)
            .bg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
        );
  
      f.render_stateful_widget(dirs_list, chunks[0], &mut app.list_state);
    }
    
    if app.search_mode {
      let search_text = Paragraph::new(Line::from(vec![
        Span::styled("Search: ", Style::default().fg(Color::Yellow)),
        Span::raw(&app.search_query),
      ]))
      .block(Block::default().borders(Borders::ALL));
      
      f.render_widget(search_text, chunks[1]);
    } else if app.bind_mode {
      let bind_text = Paragraph::new(Line::from(vec![
        Span::styled("Bind: ", Style::default().fg(Color::Green)),
        Span::raw(&app.bind_command),
      ]))
      .block(Block::default().borders(Borders::ALL));
      
      f.render_widget(bind_text, chunks[1]);
    }
  }
  
  fn get_history_file_path() -> io::Result<PathBuf> {
    let home_dir = match env::var("HOME") {
      Ok(home) => PathBuf::from(home),
      Err(_) => {
        return Err(io::Error::new(
          io::ErrorKind::NotFound,
          "HOME environment variable not set"
        ));
      }
    };
    
    let config_file = home_dir.join(".ff_config");
    
    
    if !config_file.exists() {
      File::create(&config_file)?;
    }
    
    Ok(config_file)
  }
  
  fn read_history(history_file: &Path) -> io::Result<HashMap<PathBuf, u64>> {
    let mut history = HashMap::new();
    
    if history_file.exists() {
      let file = File::open(history_file)?;
      let reader = BufReader::new(file);
      
      for line in reader.lines() {
        let line = line?;
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() == 2 {
          let path = PathBuf::from(parts[0]);
          let timestamp: u64 = parts[1].parse().unwrap_or(0);
          history.insert(path, timestamp);
        }
      }
    }
    
    Ok(history)
  }
  
  fn update_history(history_file: &Path, selected_dir: &Path) -> io::Result<()> {
    let mut history = read_history(history_file)?;
    
    
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs();
    
    history.insert(selected_dir.to_path_buf(), now);
    let mut current = selected_dir.to_path_buf();
    let mut time_decrease = 1;
    
    while let Some(parent) = current.parent() {
      let parent_path = parent.to_path_buf();
      if parent_path == current {
        break;
      }
      let parent_timestamp = history.get(&parent_path).copied().unwrap_or(0);
      let new_timestamp = now - time_decrease;
      
      if parent_timestamp < new_timestamp {
        history.insert(parent_path.clone(), new_timestamp);
      }
      
      current = parent_path;
      time_decrease += 1;
    }
    
    let mut file = OpenOptions::new()
      .write(true)
      .truncate(true)
      .create(true)
      .open(history_file)?;
    
    for (path, timestamp) in &history {
      writeln!(file, "{}|{}", path.to_string_lossy(), timestamp)?;
    }
    
    Ok(())
  }
  
  fn get_sorted_entries(dir: &Path, history: &HashMap<PathBuf, u64>, show_files: bool) -> io::Result<Vec<(PathBuf, u64, bool)>> {
    let mut entries = Vec::new();
    
    if let Ok(dir_entries) = fs::read_dir(dir) {
      for entry in dir_entries.filter_map(Result::ok) {
        let path = entry.path();
        let is_dir = path.is_dir();
        
        if show_files && is_dir {
          continue;
        }
        
        if !is_dir && !show_files {
          continue;
        }
        
        let mut score = history.get(&path).copied().unwrap_or(0);
        
        if score == 0 && !is_dir {
          if let Ok(metadata) = fs::metadata(&path) {
            if let Ok(modified) = metadata.modified() {
              if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                score = duration.as_secs();
              }
            }
          }
        }
        
        if is_dir {
          let mut current_path = dir.to_path_buf();
          while let Some(parent) = current_path.parent() {
            if path == parent {
              let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
              let boost = now - (now - score) / 4;
              score = score.max(boost);
              break;
            }
            current_path = parent.to_path_buf();
          }
        }
        
        entries.push((path, score, is_dir));
      }
    }
    
    entries.sort_by(|a, b| {
      if show_files {
        return b.1.cmp(&a.1);
      } else {
        match (a.2, b.2) {
          (true, false) => return std::cmp::Ordering::Less,
          (false, true) => return std::cmp::Ordering::Greater,
          _ => {}
        }
        
        return b.1.cmp(&a.1);
      }
    });
    
    Ok(entries)
  }
  
  fn get_scripts_file_path() -> io::Result<PathBuf> {
    let home_dir = match env::var("HOME") {
      Ok(home) => PathBuf::from(home),
      Err(_) => {
        return Err(io::Error::new(
          io::ErrorKind::NotFound,
          "HOME environment variable not set"
        ));
      }
    };
    
    Ok(home_dir.join(".ff_scripts"))
  }
  
  fn get_custom_script(dir: &Path) -> io::Result<Option<String>> {
    let scripts_file = get_scripts_file_path()?;
    
    if !scripts_file.exists() {
      return Ok(None);
    }
    
    let mut file = File::open(scripts_file)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    
    let scripts: Result<Value, _> = serde_json::from_str(&content);
    
    match scripts {
      Ok(Value::Object(map)) => {
        let dir_str = dir.to_string_lossy().to_string();
        
        for (key, value) in map {
          if key == dir_str {
            if let Value::String(cmd) = value {
              return Ok(Some(cmd));
            }
          }
        }
        
        Ok(None)
      },
      _ => Ok(None),
    }
  }
  
  fn save_custom_script(dir: &Path, command: &str) -> io::Result<()> {
    let scripts_file = get_scripts_file_path()?;
    let dir_str = dir.to_string_lossy().to_string();
    
    let content = if scripts_file.exists() {
      let mut file = File::open(&scripts_file)?;
      let mut content = String::new();
      file.read_to_string(&mut content)?;
      content
    } else {
      "{}".to_string()
    };
    
    let mut scripts: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
    
    if let Some(obj) = scripts.as_object_mut() {
      if command.is_empty() {
        obj.remove(&dir_str);
      } else {
        obj.insert(dir_str, serde_json::Value::String(command.to_string()));
      }
    }
    
    let mut file = OpenOptions::new()
      .write(true)
      .truncate(true)
      .create(true)
      .open(&scripts_file)?;
    
    let formatted = serde_json::to_string_pretty(&scripts)?;
    file.write_all(formatted.as_bytes())?;
    
    Ok(())
  }