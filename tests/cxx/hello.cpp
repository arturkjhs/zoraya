// The smallest C++ program (TASK-30): iostream, a static constructor and
// destructor, an exception thrown and caught.
#include <iostream>
#include <stdexcept>
#include <string>

int main() {
    try {
        throw std::runtime_error("caught");
    } catch (const std::exception& e) {
        std::cout << "hello from C++: " << e.what() << std::endl;
    }
    return 0;
}
