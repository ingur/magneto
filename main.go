package main

import (
	"crypto/md5"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"syscall"
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
)

type Config struct {
	Port      int      `json:"port"`
	AutoPlay  bool     `json:"autoplay"`
	Filetypes []string `json:"filetypes"`
	Playback  []string `json:"playback"`
	Fallback  []string `json:"fallback"`
	Path      string   `json:"path"`
	file      string
}

type Session struct {
	Pid  int    `json:"pid"`
	Url  string `json:"url"`
	file string
}

type TorrentInfo struct {
	Id   string
	Name string
	Time time.Time
	file *torrent.File
}

type Server struct {
	mu       sync.Mutex
	srv      *http.Server
	restart  bool
	stopChan chan os.Signal
	client   *torrent.Client
	torrents map[string]*TorrentInfo
}

type Response struct {
	Message string   `json:"message"`
	Ids     []string `json:"ids,omitempty"`
}

func Map[T, V any](ts []T, fn func(T) V) []V {
	result := make([]V, len(ts))
	for i, t := range ts {
		result[i] = fn(t)
	}
	return result
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

func getLocalIP() string {
	resp, err := net.InterfaceAddrs()
	if err != nil {
		return ""
	}

	for _, addr := range resp {
		if ipnet, ok := addr.(*net.IPNet); ok && !ipnet.IP.IsLoopback() {
			if ipnet.IP.To4() != nil {
				return ipnet.IP.String()
			}
		}
	}
	return ""
}

// Config Methods

func loadConfig() (*Config, error) {
	config := Config{
		Port:      8000,
		AutoPlay:  true,
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
	if s.Pid == 0 && s.Url == "" {
		return false
	}

	resp, err := http.Get(s.Url + "/magneto")
	if err != nil {
		fmt.Printf("Error connecting to server: %v\n", err)
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
		restart:  false,
		stopChan: make(chan os.Signal, 1),
		torrents: make(map[string]*TorrentInfo),
	}

	signal.Notify(server.stopChan, os.Interrupt, syscall.SIGTERM)

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
	http.HandleFunc("/magneto", server.ping)
	http.HandleFunc("/stop", server.stop)
	http.HandleFunc("/add", server.add)
	http.HandleFunc("/del", server.del)
	http.HandleFunc("/play", server.play)
	http.HandleFunc("/download", server.download)

	http.HandleFunc("/stream", server.stream)

	return &server
}

func (s *Server) respond(w http.ResponseWriter, res Response, code int) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	if err := json.NewEncoder(w).Encode(res); err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
	}
	log.Printf("%s: %s", http.StatusText(code), res.Message)
}

func (s *Server) start() (int, error) {
	listener, err := net.Listen("tcp", s.srv.Addr)
	if err != nil {
		return 0, err
	}

	go func() {
		if err := http.Serve(listener, nil); err != nil {
			log.Printf("HTTP server error: %v", err)
		}
	}()

	port := listener.Addr().(*net.TCPAddr).Port

	return port, nil
}

func (s *Server) ping(w http.ResponseWriter, r *http.Request) {
	w.WriteHeader(http.StatusOK)
}

func (s *Server) stop(w http.ResponseWriter, r *http.Request) {
	ip, _, _ := net.SplitHostPort(r.RemoteAddr)
	if !(ip == "127.0.0.1" || ip == "::1") {
		s.respond(w, Response{Message: "Unauthorized"}, http.StatusUnauthorized)
		return
	}

	restart := r.URL.Query().Get("restart")
	if restart == "true" {
		s.restart = true
		s.respond(w, Response{Message: "Restarting server..."}, http.StatusOK)
	} else {
		s.respond(w, Response{Message: "Stopping server..."}, http.StatusOK)
	}

	s.stopChan <- os.Interrupt
}

func (s *Server) getId(f *torrent.File) string {
	id := f.Torrent().InfoHash().String() + f.DisplayPath()
	return fmt.Sprintf("%x", md5.Sum([]byte(id)))
}

func (s *Server) addTorrent(f *torrent.File) *TorrentInfo {
	s.mu.Lock()
	defer s.mu.Unlock()

	id := s.getId(f)

	info := TorrentInfo{
		Id:   id,
		Name: f.DisplayPath(),
		Time: time.Now(),
		file: f,
	}

	s.torrents[id] = &info
	return &info
}

func (s *Server) getTorrent(id string) (*TorrentInfo, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.torrents[id]
	return t, ok
}

func (s *Server) removeTorrent(id string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.torrents, id)
}

func (s *Server) isValidFile(f *torrent.File) bool {
	ext := path.Ext(f.Path())
	return slices.Contains(config.Filetypes, ext)
}

func (s *Server) add(w http.ResponseWriter, r *http.Request) {
	uri := r.URL.Query().Get("uri")
	if uri == "" {
		s.respond(w, Response{Message: "Missing URI"}, http.StatusBadRequest)
		return
	}

	var t *torrent.Torrent
	var err error
	switch {
	case strings.HasPrefix(uri, "magnet:"):
		t, err = s.client.AddMagnet(uri)
	case strings.HasSuffix(uri, ".torrent"):
		t, err = s.client.AddTorrentFromFile(uri)
	default:
		s.respond(w, Response{Message: "Unsupported URI format"}, http.StatusBadRequest)
		return
	}
	if err != nil {
		s.respond(w, Response{Message: "Error adding torrent: " + err.Error()}, http.StatusBadRequest)
		return
	}

	log.Printf("Loading torrent info...")

	<-t.GotInfo()

	ids := make([]string, 0)
	anyValid := false
	for _, f := range t.Files() {
		if s.isValidFile(f) {
			info, exists := s.getTorrent(s.getId(f))
			if !exists {
				info = s.addTorrent(f)
			}
			ids = append(ids, info.Id)
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
		s.respond(w, Response{Message: "No valid files"}, http.StatusBadRequest)
	} else {
		s.respond(w, Response{Message: "Files added", Ids: ids}, http.StatusOK)
	}
}

func (s *Server) del(w http.ResponseWriter, r *http.Request) {
	id := r.URL.Query().Get("f")

	info, ok := s.getTorrent(id)
	if !ok {
		s.respond(w, Response{Message: "File not found"}, http.StatusNotFound)
		return
	}

	ih := info.file.Torrent().InfoHash().String()
	rel := info.file.Path()

	path := filepath.Join(config.Path, ih, rel)

	info.file.Torrent().Drop()
	s.removeTorrent(id)

	err := os.Remove(path)
	if err != nil {
		s.respond(w, Response{Message: "Error removing file"}, http.StatusInternalServerError)
		return
	}

	s.respond(w, Response{Message: "File removed"}, http.StatusOK)
}

func (s *Server) play(w http.ResponseWriter, r *http.Request) {
	ip, _, _ := net.SplitHostPort(r.RemoteAddr)
	if !(ip == "127.0.0.1" || ip == "::1") {
		s.respond(w, Response{Message: "Unauthorized"}, http.StatusUnauthorized)
		return
	}

	id := r.URL.Query().Get("f")

	_, ok := s.getTorrent(id)
	if !ok {
		s.respond(w, Response{Message: "File not found"}, http.StatusNotFound)
		return
	}

	args := append(config.Playback[1:], session.Url+"/stream?f="+id)
	cmd := exec.Command(config.Playback[0], args...)
	if err := cmd.Start(); err != nil {
		s.respond(w, Response{Message: "Error starting playback"}, http.StatusInternalServerError)
	} else {
		s.respond(w, Response{Message: "Playback started"}, http.StatusOK)
	}
}

func (s *Server) download(w http.ResponseWriter, r *http.Request) {
	id := r.URL.Query().Get("f")

	info, ok := s.getTorrent(id)
	if !ok {
		s.respond(w, Response{Message: "File not found"}, http.StatusNotFound)
		return
	}

	info.file.Download()
	s.respond(w, Response{Message: "Downloading file"}, http.StatusOK)
}

func (s *Server) stream(w http.ResponseWriter, r *http.Request) {
	id := r.URL.Query().Get("f")

	info, ok := s.getTorrent(id)
	if !ok {
		s.respond(w, Response{Message: "File not found"}, http.StatusNotFound)
		return
	}

	fn := info.file.DisplayPath()

	w.Header().Set("Expires", "0")
	w.Header().Set("Cache-Control", "no-cache, no-store, must-revalidate, max-age=0")
	w.Header().Set("Content-Disposition", "attachment; filename="+fn)

	reader := info.file.NewReader()
	reader.SetReadahead(info.file.Length() / 100)
	reader.SetResponsive()

	http.ServeContent(w, r, fn, time.Now(), reader)
}

// Main Methods
func serve() {
	server = createServer()

	port, err := server.start()
	expect(err, "Failed to start server")

	url := fmt.Sprintf("http://localhost:%d", port)
	log.Printf("Server running at %s/\n", url)

	session.Pid = os.Getpid()
	session.Url = url

	expect(session.save(), "Failed to save session")

	<-server.stopChan

	expect(server.srv.Close(), "Error closing server")
	expect(session.delete(), "Error deleting session")
	server.client.Close()

	os.RemoveAll(config.Path)
	os.MkdirAll(config.Path, os.ModePerm)

	if server.restart {
		cmd := exec.Command(ex, "serve")
		expect(cmd.Start(), "Failed to restart server")
		log.Printf("Restarting server process (Pid %d)...\n", cmd.Process.Pid)
	}
}

func start() {
	if session.exists() {
		fmt.Println("Server already running. Exiting...")
		return
	}

	cmd := exec.Command(ex, "serve")
	expect(cmd.Start(), "Failed to start server")
	fmt.Printf("Starting server process (Pid %d)...\n", cmd.Process.Pid)
	if config.Port != 0 {
		fmt.Printf("Server will be accessible at http://localhost:%d/\n", config.Port)
		fmt.Printf("and http://%s:%d/\n", getLocalIP(), config.Port)
	}
}

func stop() {
	if !session.exists() {
		fmt.Println("Server not running. Exiting...")
		session.delete()
		return
	}

	fmt.Printf("Stopping server with Pid %d\n", session.Pid)

	if _, err := http.Get(session.Url + "/stop"); err != nil {
		log.Printf("Error stopping server: %v\n", err)
	}
}

func connect(url string) {
	if url == "" {
		fmt.Println("Usage: <executable> connect <url>")
		return
	}

	resp, err := http.Get(url + "/magneto")
	if err != nil {
		fmt.Printf("Error connecting to server: %v\n", err)
		return
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusOK {
		session.Url = url
		expect(session.save(), "Failed to save session")
		fmt.Printf("Connected to server at %s\n", url)
	} else {
		fmt.Printf("Error connecting to server: %s\n", resp.Status)
	}
}

func disconnect() {
	session.delete()
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
	url := session.Url + "/add?uri=" + uri
	resp, err := http.Get(url)
	expect(err, "Error requesting play")
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusOK {
		var res Response
		if err := json.NewDecoder(resp.Body).Decode(&res); err != nil {
			fmt.Printf("Error decoding server response: %v\n", err)
		}

		fmt.Println(res.Message + ":")
		urls := Map(res.Ids, func(id string) string { return session.Url + "/stream?f=" + id })
		for i, url := range urls {
			fmt.Println(i, url)
		}

		if config.AutoPlay {
			args := append(config.Playback[1:], urls...)
			cmd := exec.Command(config.Playback[0], args...)
			expect(cmd.Start(), "Failed to start playback")
			fmt.Printf("Playing %d file(s)\n", len(urls))
		}
	} else if resp.StatusCode == http.StatusBadRequest {
		args := append(config.Fallback[1:], uri)
		cmd := exec.Command(config.Fallback[0], args...)
		expect(cmd.Start(), "Failed to open fallback")
	}
}

func main() {
	log.SetFlags(log.Ldate | log.Ltime)

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
		fmt.Println("Usage: magneto <start|stop|connect|disconnect>")
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
	case "connect":
		connect(flag.Arg(1))
	case "disconnect":
		disconnect()
	case "serve":
		serve()
	default:
		start_or_play(cmd)
	}
}
