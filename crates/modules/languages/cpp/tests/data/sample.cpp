// A shape and its area.
#include <vector>
#include <string>

namespace geometry {

template <typename T>
class Shape {
public:
    explicit Shape(std::string name) : name_(std::move(name)) {}
    virtual ~Shape() = default;
    virtual T area() const noexcept = 0;
    const std::string &name() const { return name_; }

private:
    std::string name_;
};

auto total(const std::vector<Shape<double> *> &shapes) -> double {
    double sum = 0.0;
    for (auto *shape : shapes) {
        sum += shape->area();
    }
    return sum;
}

}  // namespace geometry

int main() {
    auto lambda = [](int x) { return x * 2; };
    std::vector<int> values{1, 2, 3};
    bool empty = values.empty() || nullptr == this_is_not_a_member;
    return lambda(values.size());
}
