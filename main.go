package main

import (
	"crypto/md5"
	"embed"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"text/template"
	"time"

	"github.com/anacrolix/torrent"
	"github.com/anacrolix/torrent/storage"
)

var (
	ex      string
	exPath  string
	config  *Config
	session *Session
	server  *Server
	tmpl    *template.Template
)

//go:embed all:web
var webFS embed.FS

type Config struct {
	Port      int      `json:"port"`
	Filetypes []string `json:"filetypes"`
	Playback  []string `json:"playback"`
	Fallback  []string `json:"fallback"`
	Path      string   `json:"path"`
	file      string
}

type Session struct {
	PID  int    `json:"pid"`
	Url  string `json:"url"`
	file string
}

type TorrentInfo struct {
	Name string
	Url  string
	Time time.Time
	file *torrent.File
}

type Server struct {
	mu       sync.Mutex
	srv      *http.Server
	stopChan chan os.Signal
	client   *torrent.Client
	torrents map[string]*TorrentInfo
}

type Response struct {
	Message string   `json:"message"`
	Urls    []string `json:"urls,omitempty"`
}

func expect(err error, msg string) {
	if err != nil {
		log.Fatalf("%s: %v\n", msg, err)
	}
}

func loadJSON(file string, v interface{}) error {
	data, err := os.ReadFile(file)
	if err != nil {
		return err
	}
	err = json.Unmarshal(data, v)
	return err
}

// Config Methods

func loadConfig() (*Config, error) {
	config := Config{
		Port:      8000,
		Filetypes: []string{".mkv", ".mp4"},
		Playback:  []string{"mpv", "--no-terminal", "--force-window", "--ytdl-format=best"},
		Fallback:  []string{"qbittorrent"},
		Path:      filepath.Join(exPath, "downloads"),
		file:      filepath.Join(exPath, "config.json"),
	}

	err := os.MkdirAll(config.Path, os.ModePerm)
	expect(err, "Failed to create downloads directory")

	err = loadJSON(config.file, &config)
	if err != nil {
		if os.IsNotExist(err) {
			config.save()
			return &config, nil // It's fine if the file doesn't exist
		}
	}
	return &config, err
}

func (c *Config) save() error {
	data, err := json.MarshalIndent(c, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(c.file, data, 0644)
}

// Session Methods

func loadSession() (*Session, error) {
	file := filepath.Join(exPath, ".session")
	session := Session{file: file}

	err := loadJSON(file, &session)
	if err != nil {
		if os.IsNotExist(err) {
			return &session, nil // It's fine if the file doesn't exist
		}
	}
	return &session, err
}

func (s *Session) wait(timeout time.Duration) error {
	start := time.Now()
	for {
		if time.Since(start) > timeout {
			return fmt.Errorf("Timeout waiting for session")
		}

		if _, err := os.Stat(s.file); err == nil {
			return loadJSON(s.file, s)
		}

		time.Sleep(33 * time.Millisecond)
	}
}

func (s *Session) exists() bool {
	if s.PID == 0 || s.Url == "" {
		return false
	}

	resp, err := http.Get(s.Url + "/tori")
	if err != nil {
		log.Printf("Error pinging server: %v\n", err)
		return false
	}
	defer resp.Body.Close()

	return resp.StatusCode == http.StatusOK
}

func (s *Session) save() error {
	data, err := json.Marshal(s)
	if err != nil {
		return err
	}
	return os.WriteFile(s.file, data, 0644)
}

func (s *Session) delete() error {
	return os.Remove(s.file)
}

// Server Methods

func createServer() *Server {
	port := config.Port
	server := Server{
		srv:      &http.Server{Addr: ":" + fmt.Sprint(port)},
		stopChan: make(chan os.Signal, 1),
		torrents: make(map[string]*TorrentInfo),
	}

	os.RemoveAll(config.Path)
	err := os.MkdirAll(config.Path, os.ModePerm)
	expect(err, "Failed to create downloads directory")
	cfg := torrent.NewDefaultClientConfig()
	cfg.DefaultStorage = storage.NewFileByInfoHash(config.Path)

	// Increase default limits, no idea if this is good
	cfg.EstablishedConnsPerTorrent = 55
	cfg.HalfOpenConnsPerTorrent = 30

	client, err := torrent.NewClient(cfg)
	expect(err, "Failed to create torrent client")
	server.client = client

	// setup routes
	// TODO: restructure
	http.HandleFunc("/tori", server.ping)
	http.HandleFunc("/stop", server.stop)
	http.HandleFunc("/play", server.play)

	http.HandleFunc("/stream", server.stream)

	// http.HandleFunc("/", server.dashboard)
	// http.HandleFunc("/torrents", server.torrentinfo)

	return &server
}

func (s *Server) respond(w http.ResponseWriter, res Response) {
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(res); err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
	}
}

func (s *Server) start() (string, error) {
	listener, err := net.Listen("tcp", s.srv.Addr)
	if err != nil {
		return "", err
	}

	go func() {
		if err := http.Serve(listener, nil); err != nil {
			log.Printf("HTTP server error: %v", err)
		}
	}()

	port := listener.Addr().(*net.TCPAddr).Port
	// TODO: we also need the ipv4 address somewhere
	url := fmt.Sprintf("http://localhost:%d", port)
	return url, nil
}

func (s *Server) ping(w http.ResponseWriter, r *http.Request) {
	w.WriteHeader(http.StatusOK)
}

func (s *Server) stop(w http.ResponseWriter, r *http.Request) {
	s.respond(w, Response{Message: "Stopping server..."})
	s.stopChan <- os.Interrupt
}

func (s *Server) getKey(f *torrent.File) string {
	id := f.Torrent().InfoHash().String() + f.DisplayPath()
	return fmt.Sprintf("%x", md5.Sum([]byte(id)))
}

func (s *Server) getUrl(key string) string {
	return fmt.Sprintf("%s/stream?f=%s", session.Url, key)
}

func (s *Server) addTorrent(f *torrent.File) *TorrentInfo {
	s.mu.Lock()
	defer s.mu.Unlock()

	key := s.getKey(f)

	info := TorrentInfo{
		Name: f.DisplayPath(),
		Url:  s.getUrl(key),
		Time: time.Now(),
		file: f,
	}

	s.torrents[key] = &info
	return &info
}

func (s *Server) removeTorrent(key string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.torrents, key)
}

func (s *Server) torrentExists(t *torrent.Torrent) (bool, []string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	urls := make([]string, 0)
	for _, f := range t.Files() {
		if info, exists := s.torrents[s.getKey(f)]; exists {
			urls = append(urls, info.Url)
		}
	}
	return len(urls) > 0, urls
}

func (s *Server) isValidFile(f *torrent.File) bool {
	ext := path.Ext(f.Path())
	return slices.Contains(config.Filetypes, ext)
}

func (s *Server) play(w http.ResponseWriter, r *http.Request) {
	uri := r.URL.Query().Get("uri")
	if uri == "" {
		http.Error(w, "Missing URI", http.StatusBadRequest)
		return
	}

	var t *torrent.Torrent
	switch {
	case strings.HasPrefix(uri, "magnet:"):
		t, _ = s.client.AddMagnet(uri)
	case strings.HasSuffix(uri, ".torrent"):
		t, _ = s.client.AddTorrentFromFile(uri)
	default:
		http.Error(w, "Unsupported URI format", http.StatusBadRequest)
		return
	}

	log.Printf("Loading torrent info...")

	<-t.GotInfo()

	if exists, urls := s.torrentExists(t); exists {
		s.respond(w, Response{Message: "Torrent already added", Urls: urls})
		return
	}

	urls := make([]string, 0)
	anyValid := false
	for _, f := range t.Files() {
		if s.isValidFile(f) {
			info := s.addTorrent(f)
			urls = append(urls, info.Url)
			anyValid = true

			// download first and last pieces first to start streaming asap (in theory)
			t.Piece(f.EndPieceIndex() - 1).SetPriority(torrent.PiecePriorityNow)
			t.Piece(f.BeginPieceIndex()).SetPriority(torrent.PiecePriorityNow)
		} else {
			f.SetPriority(torrent.PiecePriorityNone)
		}
	}

	if !anyValid {
		t.Drop()
		http.Error(w, "No valid files", http.StatusBadRequest)
	} else {
		s.respond(w, Response{Message: "Torrent added", Urls: urls})
		fmt.Printf("Torrent added: %s\n", urls)
	}
}

func (s *Server) stream(w http.ResponseWriter, r *http.Request) {
	key := r.URL.Query().Get("f")
	key = strings.TrimSpace(key)
	key = strings.ReplaceAll(key, "\n", "")

	t, ok := s.torrents[key]
	if !ok {
		http.Error(w, "File not found", http.StatusNotFound)
		return
	}

	fn := t.file.DisplayPath()

	w.Header().Set("Expires", "0")
	w.Header().Set("Cache-Control", "no-cache, no-store, must-revalidate, max-age=0")
	w.Header().Set("Content-Disposition", "attachment; filename="+fn)

	reader := t.file.NewReader()
	reader.SetReadahead(t.file.Length() / 100)
	reader.SetResponsive()

	http.ServeContent(w, r, fn, time.Now(), reader)
}

// TODO: preload templates
func (s *Server) dashboard(w http.ResponseWriter, r *http.Request) {
	t, err := template.ParseFiles("web/dashboard.html")
	expect(err, "Failed to parse template")
	t.Execute(w, nil)
}

// TODO: rename/restructure this
// func (s *Server) torrentinfo(w http.ResponseWriter, r *http.Request) {
// 	t, err := template.ParseFiles("web/torrents.html")
// 	expect(err, "Failed to parse template")
//
// 	res := make([]TorrentInfo, 0)
// 	for _, f := range s.torrents {
// 		info := TorrentInfo{}
// 		info.Name = f.DisplayPath()
// 		info.Date = "today"
// 		info.Progress = int(f.BytesCompleted() * 100 / f.Length())
// 		res = append(res, info)
// 	}
//
// 	log.Printf("Torrents: %v\n", res)
//
// 	err = t.Execute(w, res)
// 	if err != nil {
// 		log.Printf("Error executing template: %v", err)
// 	}
// }

// Main Methods

func start() {
	if session.exists() {
		log.Println("Server already running. Exiting...")
		return
	}

	cmd := exec.Command(ex, "serve")
	expect(cmd.Start(), "Failed to start server")
	log.Printf("Started server with PID %d\n", cmd.Process.Pid)
}

func stop() {
	if !session.exists() {
		log.Println("Server not running. Exiting...")
		session.delete()
		return
	}

	log.Printf("Stopping server with PID %d\n", session.PID)

	if _, err := http.Get(session.Url + "/stop"); err != nil {
		log.Printf("Error stopping server: %v\n", err)
	}
}

func serve() {
	server = createServer()

	// var err error
	// tmpl, err = template.ParseFS(webFS, "index.html")
	// expect(err, "Failed to parse template")

	url, err := server.start()
	expect(err, "Failed to start server")

	session.PID = os.Getpid()
	session.Url = url

	expect(session.save(), "Failed to save session")

	log.Printf("Server running at %s\n", url)

	<-server.stopChan

	expect(server.srv.Close(), "Error closing server")
	expect(session.delete(), "Error deleting session")
	server.client.Close()

	os.RemoveAll(config.Path)
	os.MkdirAll(config.Path, os.ModePerm)
}

func start_or_play(uri string) {
	if session.exists() {
		play(uri)
	} else {
		start()
		err := session.wait(5 * time.Second)
		expect(err, "Failed to load session")
		play(uri)
	}
}

func play(uri string) {
	url := session.Url + "/play?uri=" + uri
	resp, err := http.Get(url)
	expect(err, "Error requesting play")
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusOK {
		var res Response
		if err := json.NewDecoder(resp.Body).Decode(&res); err != nil {
			log.Printf("Error decoding response: %v\n", err)
		}

		args := append(config.Playback[1:], res.Urls...)
		cmd := exec.Command(config.Playback[0], args...)
		expect(cmd.Start(), "Failed to start playback")

		fmt.Printf("Started playback with PID %d\n", cmd.Process.Pid)
	} else if resp.StatusCode == http.StatusBadRequest {

		args := append(config.Fallback[1:], uri)
		cmd := exec.Command(config.Fallback[0], args...)
		expect(cmd.Start(), "Failed to open fallback")
	}
}

func main() {
	log.SetFlags(log.LstdFlags | log.Lshortfile) // Configure log package to show file name and line number

	flag.Parse()

	// get executable path
	var err error
	ex, err = os.Executable()
	expect(err, "Failed to get executable path")
	exPath = filepath.Dir(ex)

	// load config
	config, err = loadConfig()
	expect(err, "Failed to load config")

	cmd := flag.Arg(0)
	if cmd == "" {
		log.Println("Usage: <executable> <start|stop>")
		return
	}

	// validate that session still actually exists
	session, err = loadSession()
	expect(err, "Failed to load session")

	// handle commands
	switch cmd {
	case "start":
		start()
	case "stop":
		stop()
	case "serve":
		serve()
	default:
		start_or_play(cmd)
	}
}
