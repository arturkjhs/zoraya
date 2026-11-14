import sys
import zsample

assert zsample.mul_add(6, 7, 0) == 42
assert zsample.mul_add(2, 3, 4) == 10
assert "zsample" in sys.builtin_module_names
print("ok zsample", zsample.mul_add(6, 7, 0))
try:
    zsample.mul_add(1, 2)
except TypeError:
    print("ok typeerror")
print("ext done")
