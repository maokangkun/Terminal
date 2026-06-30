"""Formula parsing and numpy evaluation backed by SymPy."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable

import numpy as np
import sympy as sp
from sympy.parsing.sympy_parser import (
    convert_xor,
    implicit_multiplication_application,
    parse_expr,
    standard_transformations,
)

try:
    from sympy.parsing.latex import parse_latex
except ImportError:
    parse_latex = None

try:
    from .settings import DEFAULT_FORMULA
except ImportError:
    from settings import DEFAULT_FORMULA


DEFAULT_X, DEFAULT_Y, DEFAULT_Z = sp.symbols("x y z")
TRANSFORMATIONS = standard_transformations + (implicit_multiplication_application, convert_xor)
SYMPY_LOCALS = {
    "x": DEFAULT_X,
    "y": DEFAULT_Y,
    "z": DEFAULT_Z,
    "pi": sp.pi,
    "e": sp.E,
    "E": sp.E,
    "sin": sp.sin,
    "cos": sp.cos,
    "tan": sp.tan,
    "sqrt": sp.sqrt,
    "exp": sp.exp,
    "log": sp.log,
    "abs": sp.Abs,
}
BRACED_FUNCTIONS = ("sqrt", "sin", "cos", "tan", "log", "exp", "abs")


@dataclass(frozen=True)
class ParsedFormula:
    mode: str
    target_symbol: sp.Symbol | None
    input_symbols: tuple[sp.Symbol, ...]
    expression: sp.Expr

    @property
    def dimension(self) -> int:
        return len(self.input_symbols)


@dataclass
class FormulaModel:
    formula: str = DEFAULT_FORMULA
    error: str | None = None
    mode: str = field(default="explicit", init=False)
    target_symbol: sp.Symbol | None = field(default=DEFAULT_Z, init=False)
    input_symbols: tuple[sp.Symbol, ...] = field(default=(DEFAULT_X, DEFAULT_Y), init=False)
    expression: sp.Expr | None = field(default=None, init=False)
    _function: Callable[..., np.ndarray] | None = field(default=None, init=False, repr=False)

    def __post_init__(self) -> None:
        self.set_formula(self.formula)

    def set_formula(self, formula: str) -> bool:
        try:
            parsed = parse_formula(formula)
            function = sp.lambdify(parsed.input_symbols, parsed.expression, modules="numpy")
        except Exception as exc:
            self.error = str(exc)
            return False
        self.formula = formula
        self.mode = parsed.mode
        self.target_symbol = parsed.target_symbol
        self.input_symbols = parsed.input_symbols
        self.expression = parsed.expression
        self._function = function
        self.error = None
        return True

    @property
    def is_2d(self) -> bool:
        return self.is_implicit_2d or len(self.input_symbols) <= 1

    @property
    def is_implicit_2d(self) -> bool:
        return self.mode == "implicit2d"

    @property
    def target_label(self) -> str:
        return "0" if self.target_symbol is None else str(self.target_symbol)

    @property
    def input_labels(self) -> tuple[str, ...]:
        return tuple(str(symbol) for symbol in self.input_symbols)

    def evaluate(self, x: np.ndarray, y: np.ndarray) -> np.ndarray:
        if self._function is None:
            self.set_formula(self.formula)
        if self._function is None:
            z = np.zeros_like(x, dtype=float)
        else:
            with np.errstate(all="ignore"):
                z = self._function(x, y)
        if np.isscalar(z):
            z = np.full_like(x, float(z), dtype=float)
        return np.nan_to_num(np.asarray(z, dtype=float), nan=0.0, posinf=0.0, neginf=0.0)

    def evaluate_curve(self, x: np.ndarray) -> np.ndarray:
        if self._function is None:
            self.set_formula(self.formula)
        if self._function is None:
            y = np.zeros_like(x, dtype=float)
        else:
            with np.errstate(all="ignore"):
                y = self._function(x) if self.input_symbols else self._function()
        if np.isscalar(y):
            y = np.full_like(x, float(y), dtype=float)
        return np.nan_to_num(np.asarray(y, dtype=float), nan=0.0, posinf=0.0, neginf=0.0)

    def evaluate_implicit(self, x: np.ndarray, y: np.ndarray) -> np.ndarray:
        if self._function is None:
            self.set_formula(self.formula)
        if self._function is None:
            values = np.zeros_like(x, dtype=float)
        else:
            with np.errstate(all="ignore"):
                values = self._function(x, y)
        if np.isscalar(values):
            values = np.full_like(x, float(values), dtype=float)
        return np.nan_to_num(np.asarray(values, dtype=float), nan=np.nan, posinf=np.nan, neginf=np.nan)


def parse_formula(formula: str) -> ParsedFormula:
    text = formula.strip().replace("−", "-").strip("$")
    if "=" not in text:
        return _parsed_explicit(None, _parse_math_text(text))

    left_text, right_text = text.split("=", 1)
    left = _parse_math_text(left_text.strip().strip("$"))
    right = _parse_math_text(right_text.strip().strip("$"))
    if isinstance(left, sp.Symbol) and left not in right.free_symbols:
        return _parsed_explicit(left, right)
    return _parsed_implicit(left - right)


def compile_expression(formula: str) -> str:
    """Compatibility helper for tests and quick inspection."""
    parsed = parse_formula(formula)
    args = ", ".join(str(symbol) for symbol in parsed.input_symbols)
    if parsed.mode == "implicit2d":
        return f"F({args})={parsed.expression}=0"
    return f"{parsed.target_symbol}({args})={parsed.expression}"


def _parse_math_text(text: str) -> sp.Expr:
    if parse_latex is not None and _looks_like_latex(text):
        try:
            return parse_latex(text)
        except Exception:
            pass
    cleaned = _latex_to_sympy_text(text)
    return parse_expr(cleaned, local_dict=SYMPY_LOCALS, transformations=TRANSFORMATIONS, evaluate=False)


def _parsed_explicit(target_symbol: sp.Symbol | None, expression: sp.Expr) -> ParsedFormula:
    free_symbols = set(expression.free_symbols)
    if target_symbol is not None and target_symbol in free_symbols:
        raise ValueError(f"right side cannot depend on output variable {target_symbol}")
    input_symbols = tuple(sorted(free_symbols, key=lambda symbol: symbol.name))
    if len(input_symbols) > 2:
        names = ", ".join(str(symbol) for symbol in input_symbols)
        raise ValueError(f"only one or two input variables are supported, got: {names}")
    if target_symbol is None:
        target_symbol = DEFAULT_Z if len(input_symbols) == 2 else DEFAULT_Y
        if target_symbol in input_symbols:
            target_symbol = sp.Symbol("f")
    return ParsedFormula(
        mode="explicit",
        target_symbol=target_symbol,
        input_symbols=input_symbols,
        expression=expression,
    )


def _parsed_implicit(expression: sp.Expr) -> ParsedFormula:
    input_symbols = tuple(sorted(expression.free_symbols, key=lambda symbol: symbol.name))
    if len(input_symbols) != 2:
        names = ", ".join(str(symbol) for symbol in input_symbols) or "none"
        raise ValueError(f"implicit equations need exactly two variables, got: {names}")
    return ParsedFormula(
        mode="implicit2d",
        target_symbol=None,
        input_symbols=input_symbols,
        expression=expression,
    )


def _looks_like_latex(expr: str) -> bool:
    return "\\" in expr or "{" in expr or "}" in expr


def _latex_to_sympy_text(expr: str) -> str:
    expr = expr.strip().strip("$")
    expr = expr.replace("\\left", "").replace("\\right", "")
    expr = expr.replace("\\cdot", "*").replace("\\times", "*")
    expr = _replace_latex_fractions(expr)
    expr = _replace_latex_exp(expr)
    for name in BRACED_FUNCTIONS:
        expr = _replace_braced_function(expr, rf"\{name}", name)
    expr = expr.replace("\\", "")
    expr = expr.replace("{", "(").replace("}", ")")
    return expr


def _replace_latex_fractions(expr: str) -> str:
    while True:
        start = expr.find(r"\frac")
        if start == -1:
            return expr
        first_open = _next_non_space(expr, start + len(r"\frac"))
        if first_open >= len(expr) or expr[first_open] != "{":
            return expr
        numerator, first_close = _read_braced(expr, first_open)
        second_open = _next_non_space(expr, first_close + 1)
        if second_open >= len(expr) or expr[second_open] != "{":
            return expr
        denominator, second_close = _read_braced(expr, second_open)
        replacement = f"(({numerator})/({denominator}))"
        expr = expr[:start] + replacement + expr[second_close + 1 :]


def _replace_latex_exp(expr: str) -> str:
    while True:
        start = expr.find("e^{")
        if start == -1:
            return expr
        content, close = _read_braced(expr, start + 2)
        expr = expr[:start] + f"exp({content})" + expr[close + 1 :]


def _replace_braced_function(expr: str, latex_name: str, sympy_name: str) -> str:
    while True:
        start = expr.find(latex_name + "{")
        if start == -1:
            return expr
        content, close = _read_braced(expr, start + len(latex_name))
        expr = expr[:start] + f"{sympy_name}({content})" + expr[close + 1 :]


def _next_non_space(expr: str, index: int) -> int:
    while index < len(expr) and expr[index].isspace():
        index += 1
    return index


def _read_braced(expr: str, open_index: int) -> tuple[str, int]:
    depth = 0
    for index in range(open_index, len(expr)):
        if expr[index] == "{":
            depth += 1
        elif expr[index] == "}":
            depth -= 1
            if depth == 0:
                return expr[open_index + 1 : index], index
    raise ValueError("unclosed brace in formula")
