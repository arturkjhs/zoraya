// The smallest C++ program (TASK-30): iostream, a static constructor and
// destructor, an exception thrown and caught.
#include <iostream>
#include <stdexcept>
#include <string>

// Built without -std=: zc++'s default is C++23 (ADR-0364, INV-0364-1).
static_assert(__cplusplus == 202302L, "zc++'s default standard is C++23");

int main() {
    try {
        throw std::runtime_error("caught");
    } catch (const std::exception& e) {
        std::cout << "hello from C++: " << e.what() << std::endl;
    }
    return 0;
}
