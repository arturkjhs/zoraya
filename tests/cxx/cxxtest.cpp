// The C++ runtime inside Zoraya (TASK-30, proposed ADR-0360 to ADR-0366):
// libc++, libc++abi and libunwind, built by zc++, run from the volume.
// `cxxtest MODE [ARG...]` runs one group and prints "cxxtest: MODE passed", or
// "cxxtest: FAIL: ..." and exits 1. tests/cxx/cxx.script runs every mode.
#include <algorithm>
#include <atomic>
#include <cassert>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <filesystem>
#include <fstream>
#include <functional>
#include <future>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <new>
#include <queue>
#include <random>
#include <regex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <system_error>
#include <thread>
#include <typeinfo>
#include <vector>

namespace {

[[noreturn]] void fail(const std::string& what) {
    std::printf("cxxtest: FAIL: %s\n", what.c_str());
    std::fflush(stdout);
    std::exit(1);
}

void check(bool ok, const std::string& what) {
    if (!ok)
        fail(what);
}

// ---------------------------------------------------------------- exceptions

int live = 0; // objects alive, so that unwinding is seen to run destructors

struct Guard {
    Guard() { ++live; }
    ~Guard() { --live; }
};

struct Error : std::runtime_error {
    int depth;
    Error(int d) : std::runtime_error("deep error"), depth(d) {}
};

[[gnu::noinline]] void deep(int n) {
    Guard g;
    if (n == 0)
        throw Error(live);
    deep(n - 1);
}

struct ThrowsInCtor {
    Guard member;
    ThrowsInCtor() { throw std::logic_error("constructor"); }
};

void exceptions() {
    // Thrown 20 frames down, caught here; every frame's destructor ran.
    try {
        deep(20);
        fail("deep() returned");
    } catch (const Error& e) {
        check(e.depth == 21, "21 guards alive at the throw");
        check(live == 0, "unwinding ran every destructor");
        check(std::strcmp(e.what(), "deep error") == 0, "what()");
    }
    // Rethrow, catch by base, catch(...).
    try {
        try {
            throw std::out_of_range("range");
        } catch (const std::logic_error&) {
            throw;
        }
    } catch (const std::exception& e) {
        check(std::string(e.what()) == "range", "rethrown by base");
    }
    bool any = false;
    try {
        throw 42;
    } catch (...) {
        any = true;
    }
    check(any, "catch (...)");
    // A member constructed before the constructor threw is destroyed.
    try {
        ThrowsInCtor t;
        fail("constructed");
    } catch (const std::logic_error&) {
        check(live == 0, "member destroyed after the constructor threw");
    }
    // The library's own: bad_alloc from new of a size no heap gives,
    // out_of_range from at(), system_error from a std::thread call.
    // The size is volatile and the block is used: an allocation the
    // compiler can see unused may be left out (C++14 [expr.new]).
    volatile std::size_t huge = static_cast<std::size_t>(1) << 62;
    try {
        char* p = new char[huge];
        p[0] = 1;
        std::printf("cxxtest: %p\n", static_cast<void*>(p));
        fail("2^62 bytes allocated");
    } catch (const std::bad_alloc&) {
    }
    try {
        std::vector<int> v(3);
        (void)v.at(3);
        fail("at(3) of 3");
    } catch (const std::out_of_range&) {
    }
    try {
        std::thread t;
        t.join();
        fail("joined a thread that is not one");
    } catch (const std::system_error& e) {
        check(e.code() == std::errc::invalid_argument, "join of a non-thread: invalid_argument");
    }
    // An exception carried out of a std::thread by exception_ptr.
    std::exception_ptr carried;
    std::thread t([&] {
        try {
            deep(5);
        } catch (...) {
            carried = std::current_exception();
        }
    });
    t.join();
    check(carried != nullptr, "an exception_ptr came back from the thread");
    try {
        std::rethrow_exception(carried);
    } catch (const Error& e) {
        check(e.depth == 6, "the thread's exception, rethrown here");
    }
    // The same through std::async and a future.
    auto f = std::async(std::launch::async, [] { return std::stoi("not a number"); });
    try {
        (void)f.get();
        fail("stoi of a word");
    } catch (const std::invalid_argument&) {
    }
    // std::uncaught_exceptions during unwinding.
    struct Counts {
        int* seen;
        ~Counts() { *seen = std::uncaught_exceptions(); }
    };
    int seen = -1;
    try {
        Counts c{&seen};
        throw 1;
    } catch (int) {
    }
    check(seen == 1, "uncaught_exceptions() is 1 while unwinding");
}

// ---------------------------------------------------------------- RTTI

struct Base {
    virtual ~Base() = default;
};
struct Left : virtual Base {
    int l = 1;
};
struct Right : virtual Base {
    int r = 2;
};
struct Both : Left, Right {};
struct Other : Base {};

void rtti() {
    Both both;
    Base* b = &both;
    check(typeid(*b) == typeid(Both), "typeid of the dynamic type");
    check(typeid(*b) != typeid(Base), "typeid differs from the static type");
    check(std::string(typeid(int).name()) == "i", "typeid(int).name()");
    check(dynamic_cast<Both*>(b) == &both, "downcast through a virtual base");
    Left* l = &both;
    Right* r = dynamic_cast<Right*>(l);
    check(r != nullptr && r->r == 2, "cross cast Left to Right");
    check(dynamic_cast<Other*>(b) == nullptr, "a failed pointer cast is null");
    try {
        (void)dynamic_cast<Other&>(*b);
        fail("a failed reference cast returned");
    } catch (const std::bad_cast&) {
    }
    Base* none = nullptr;
    try {
        (void)typeid(*none);
        fail("typeid of a null pointer returned");
    } catch (const std::bad_typeid&) {
    }
    // A catch clause matches by type info across the hierarchy.
    try {
        throw Both();
    } catch (const Right& caught) {
        check(caught.r == 2, "caught by a second base");
    }
    std::type_info const& ti = typeid(std::map<std::string, int>);
    check(ti.hash_code() == typeid(std::map<std::string, int>).hash_code(), "hash_code is stable");
}

// ---------------------------------------------------------------- static constructors and destructors

std::string order; // filled by the constructors below, in definition order

struct Ordered {
    const char* name;
    explicit Ordered(const char* n) : name(n) { order += n; }
    ~Ordered() { std::printf("cxxtest: destroyed %s\n", name); }
};
Ordered first("a");
Ordered second("b");
Ordered third("c");

void statics() {
    check(order == "abc", "static constructors ran in definition order: " + order);
    // A function-local static is constructed once, by one thread.
    static std::atomic<int> made{0};
    struct Once {
        Once() { made.fetch_add(1); }
    };
    std::vector<std::thread> ts;
    for (int i = 0; i < 4; ++i)
        ts.emplace_back([] { static Once o; });
    for (auto& t : ts)
        t.join();
    check(made.load() == 1, "a local static constructed once");
    // The destructors run at exit in reverse order: the script reads c, b, a.
    std::atexit([] { std::printf("cxxtest: atexit ran\n"); });
}

// ---------------------------------------------------------------- threads

void threads() {
    // mutex: four threads, 10000 increments each.
    std::mutex m;
    long counter = 0;
    std::vector<std::thread> ts;
    for (int i = 0; i < 4; ++i)
        ts.emplace_back([&] {
            for (int j = 0; j < 10000; ++j) {
                std::lock_guard<std::mutex> g(m);
                ++counter;
            }
        });
    for (auto& t : ts)
        t.join();
    check(counter == 40000, "mutex: 40000 increments");
    // condition_variable: a bounded queue between a producer and a consumer.
    std::condition_variable cv;
    std::queue<int> q;
    bool done = false;
    long sum = 0;
    std::thread consumer([&] {
        std::unique_lock<std::mutex> g(m);
        for (;;) {
            cv.wait(g, [&] { return !q.empty() || done; });
            while (!q.empty()) {
                sum += q.front();
                q.pop();
            }
            if (done)
                return;
            cv.notify_all();
        }
    });
    for (int i = 1; i <= 1000; ++i) {
        std::unique_lock<std::mutex> g(m);
        cv.wait(g, [&] { return q.size() < 8; });
        q.push(i);
        cv.notify_all();
    }
    {
        std::lock_guard<std::mutex> g(m);
        done = true;
    }
    cv.notify_all();
    consumer.join();
    check(sum == 500500, "condition_variable: the sum of 1..1000");
    // A timed wait that times out.
    {
        std::unique_lock<std::mutex> g(m);
        auto t0 = std::chrono::steady_clock::now();
        bool woke = cv.wait_for(g, std::chrono::milliseconds(50), [] { return false; });
        auto waited = std::chrono::steady_clock::now() - t0;
        check(!woke, "wait_for with a false predicate times out");
        check(waited >= std::chrono::milliseconds(50), "wait_for waited 50 ms");
    }
    // future and promise, packaged_task, async.
    std::promise<int> p;
    std::future<int> f = p.get_future();
    std::thread setter([&] {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
        p.set_value(7);
    });
    check(f.get() == 7, "promise and future");
    setter.join();
    std::packaged_task<int(int, int)> task([](int a, int b) { return a * b; });
    auto tf = task.get_future();
    std::thread tt(std::move(task), 6, 7);
    tt.join();
    check(tf.get() == 42, "packaged_task");
    std::vector<std::future<long>> parts;
    for (int i = 0; i < 4; ++i)
        parts.push_back(std::async(std::launch::async, [i] {
            long s = 0;
            for (int j = i * 250 + 1; j <= (i + 1) * 250; ++j)
                s += j;
            return s;
        }));
    long total = 0;
    for (auto& part : parts)
        total += part.get();
    check(total == 500500, "async: four parts of 1..1000");
    // Thread ids, hardware_concurrency, yield, atomics' wait and notify.
    check(std::this_thread::get_id() != std::thread::id(), "this thread has an id");
    std::this_thread::yield();
    std::atomic<int> flag{0};
    std::thread waker([&] {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
        flag.store(1);
        flag.notify_one();
    });
    flag.wait(0);
    waker.join();
    check(flag.load() == 1, "atomic wait and notify");
}

// ---------------------------------------------------------------- thread_local

std::atomic<int> tls_destroyed{0};
std::atomic<int> tls_made{0};

struct PerThread {
    int value = 0;
    PerThread() { tls_made.fetch_add(1); }
    ~PerThread() { tls_destroyed.fetch_add(1); }
};
thread_local PerThread per_thread;
thread_local int plain = 5;

void tls() {
    std::vector<std::thread> ts;
    std::atomic<int> sum{0};
    for (int i = 0; i < 4; ++i)
        ts.emplace_back([&, i] {
            per_thread.value = i + 1;
            plain += i;
            std::this_thread::sleep_for(std::chrono::milliseconds(5));
            sum.fetch_add(per_thread.value * 10 + plain);
        });
    for (auto& t : ts)
        t.join();
    // (1..4)*10 + (5+0 .. 5+3) = 100 + 26.
    check(sum.load() == 126, "each thread has its own thread_local values");
    check(tls_made.load() == 4, "four thread_local objects made");
    check(tls_destroyed.load() == 4, "four thread_local destructors ran at thread exit (__cxa_thread_atexit)");
    check(plain == 5, "the main thread's value is its own");
}

// ---------------------------------------------------------------- iostream

void iostreams(const char* dir) {
    std::cout << "cxxtest: cout " << 42 << ' ' << 2.5 << ' ' << std::hex << 255 << std::dec << std::endl;
    std::cerr << "cxxtest: cerr line" << std::endl;
    std::ostringstream os;
    os << std::fixed;
    os.precision(3);
    os << 3.14159 << '|' << std::boolalpha << true;
    check(os.str() == "3.142|true", "ostringstream: " + os.str());
    std::istringstream is("12 abc 4.5");
    int n;
    std::string w;
    double d;
    is >> n >> w >> d;
    check(n == 12 && w == "abc" && d == 4.5, "istringstream");
    // A file written and read back through fstream.
    std::string file = std::string(dir) + "/cxxtest.txt";
    {
        std::ofstream out(file);
        check(out.good(), "ofstream opened " + file);
        for (int i = 0; i < 100; ++i)
            out << "line " << i << '\n';
    }
    std::ifstream in(file);
    check(in.good(), "ifstream opened " + file);
    std::string line, last;
    int lines = 0;
    while (std::getline(in, line)) {
        ++lines;
        last = line;
    }
    check(lines == 100 && last == "line 99", "100 lines read back");
    in.close();
    std::filesystem::remove(file);
    // The C locale, by name and as the default (ADR-0363).
    check(std::locale().name() == "C", "the global locale is C");
    check(std::locale("C").name() == "C", "the C locale by name");
    std::string posix = std::locale("POSIX").name();
    check(posix == "POSIX" || posix == "C", "the POSIX locale by name: " + posix);
    try {
        std::locale named("de_DE.UTF-8");
        // musl takes any name and gives the C locale's rules under it (with
        // UTF-8 for the multibyte functions): the decimal point stays '.'.
        std::printf("cxxtest: locale de_DE.UTF-8 named %s, decimal point %c\n", named.name().c_str(),
                    std::use_facet<std::numpunct<char>>(named).decimal_point());
    } catch (const std::runtime_error&) {
        std::printf("cxxtest: locale de_DE.UTF-8 refused\n");
    }
    // regex and std::format-free formatting of numbers.
    std::regex re("([a-z]+)-([0-9]+)");
    std::smatch m;
    std::string text = "item-42";
    check(std::regex_match(text, m, re) && m[2] == "42", "regex");
    check(std::to_string(1.5) == "1.500000", "to_string");
    // random_device is getentropy over the random service (patch llvm/0001).
    std::random_device rd;
    unsigned a = rd(), b = rd(), c = rd();
    check(!(a == b && b == c), "random_device gives different numbers");
}

// ---------------------------------------------------------------- filesystem

int filesystem(int argc, char** argv) {
    // cxxtest fs DIR...: lists each directory, one line an entry, sorted, or
    // the error std::filesystem reports.
    int status = 0;
    for (int i = 2; i < argc; ++i) {
        std::error_code ec;
        std::vector<std::string> names;
        for (auto it = std::filesystem::directory_iterator(argv[i], ec); !ec && it != std::filesystem::directory_iterator();
             it.increment(ec))
            names.push_back(it->path().filename().string() + (it->is_directory() ? "/" : ""));
        if (ec) {
            std::printf("cxxtest: %s: %s\n", argv[i], ec.message().c_str());
            status = 1;
            continue;
        }
        std::sort(names.begin(), names.end());
        std::printf("cxxtest: %s:", argv[i]);
        for (auto& n : names)
            std::printf(" %s", n.c_str());
        std::printf("\n");
    }
    return status;
}

void fsops(const char* dir) {
    namespace fs = std::filesystem;
    fs::path base = fs::path(dir) / "cxxtest-fs";
    fs::remove_all(base);
    fs::create_directories(base / "a" / "b");
    { std::ofstream(base / "a" / "f.txt") << "twelve bytes"; }
    check(fs::exists(base / "a" / "b") && fs::is_directory(base / "a" / "b"), "create_directories");
    check(fs::file_size(base / "a" / "f.txt") == 12, "file_size");
    fs::copy_file(base / "a" / "f.txt", base / "g.txt");
    check(fs::file_size(base / "g.txt") == 12, "copy_file (the fstream copy, patch llvm/0002)");
    fs::rename(base / "g.txt", base / "h.txt");
    check(!fs::exists(base / "g.txt") && fs::exists(base / "h.txt"), "rename");
    fs::resize_file(base / "h.txt", 4);
    check(fs::file_size(base / "h.txt") == 4, "resize_file (truncate over ftruncate)");
    int count = 0;
    for (auto& e : fs::recursive_directory_iterator(base)) {
        (void)e;
        ++count;
    }
    check(count == 4, "recursive_directory_iterator: a, a/b, a/f.txt, h.txt");
    std::error_code ec;
    fs::create_hard_link(base / "h.txt", base / "link", ec);
    check(ec == std::errc::not_supported, "create_hard_link: not supported (ADR-0013)");
    (void)fs::space(base, ec);
    check(ec == std::errc::not_supported, "space: not supported");
    check(!fs::current_path().empty(), "current_path");
    check(fs::remove_all(base) == 5, "remove_all removed five");
}

} // namespace

int main(int argc, char** argv) {
    if (argc < 2) {
        std::fprintf(stderr, "usage: cxxtest MODE [ARG...]\n");
        return 2;
    }
    std::string mode = argv[1];
    if (mode == "exceptions")
        exceptions();
    else if (mode == "rtti")
        rtti();
    else if (mode == "statics")
        statics();
    else if (mode == "threads")
        threads();
    else if (mode == "tls")
        tls();
    else if (mode == "iostream" && argc == 3)
        iostreams(argv[2]);
    else if (mode == "fsops" && argc == 3)
        fsops(argv[2]);
    else if (mode == "fs")
        return filesystem(argc, argv);
    else if (mode == "uncaught")
        throw std::runtime_error("nobody catches this");
    else if (mode == "noexcept") {
        // The throw is in another function, so that the compiler sees no
        // throw in the noexcept one (-Wexceptions).
        auto thrower = [](int n) {
            if (n > 0)
                throw std::logic_error("out of a noexcept function");
        };
        auto f = [&](int n) noexcept { thrower(n); };
        f(argc);
    } else if (mode == "terminate")
        std::terminate();
    else if (mode == "hardening") {
        std::vector<int> v(2);
        std::printf("cxxtest: about to read v[2]\n");
        std::fflush(stdout);
        return v[2 + (argc > 1000)];
    } else {
        std::fprintf(stderr, "cxxtest: unknown mode %s\n", mode.c_str());
        return 2;
    }
    std::printf("cxxtest: %s passed\n", mode.c_str());
    return 0;
}
