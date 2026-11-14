/* A built-in module of python3, added by the build hook: zsample.mul_add(a, b, c)
   is a * b + c, computed by a function of a prebuilt static library. */
#define PY_SSIZE_T_CLEAN
#include <Python.h>

#include "zhelper.h"

static PyObject *zsample_mul_add(PyObject *self, PyObject *args) {
    long a, b, c;
    (void)self;
    if (!PyArg_ParseTuple(args, "lll", &a, &b, &c)) return NULL;
    return PyLong_FromLong(zhelper_mul_add(a, b, c));
}

static PyMethodDef methods[] = {
    {"mul_add", zsample_mul_add, METH_VARARGS, "a * b + c"},
    {NULL, NULL, 0, NULL},
};

static struct PyModuleDef module = {PyModuleDef_HEAD_INIT, "zsample", "sample built-in module", -1, methods};

PyMODINIT_FUNC PyInit_zsample(void) { return PyModule_Create(&module); }
