package app.valanium;

import java.math.BigDecimal;
import java.math.MathContext;

/** Small arithmetic parser for the message composer; never evaluates executable code. */
final class Calculator {
    private final String expression;
    private int at;

    private Calculator(String source) {
        expression = source.replace(',', '.').replaceAll("\\s+", "");
        if (expression.isEmpty() || expression.length() > 120) throw new IllegalArgumentException("Проверьте выражение");
    }

    static String evaluate(String source) {
        Calculator parser = new Calculator(source);
        double value = parser.sum();
        if (parser.at != parser.expression.length() || !Double.isFinite(value))
            throw new IllegalArgumentException("Проверьте выражение");
        return BigDecimal.valueOf(value).round(new MathContext(12))
                .stripTrailingZeros().toPlainString();
    }

    private double sum() {
        double value = product();
        while (at < expression.length()) {
            char op = expression.charAt(at);
            if (op != '+' && op != '-') break;
            at++;
            value += (op == '+' ? 1 : -1) * product();
        }
        return value;
    }

    private double product() {
        double value = factor();
        while (at < expression.length()) {
            char op = expression.charAt(at);
            if (op != '*' && op != '/') break;
            at++;
            double right = factor();
            if (op == '/' && right == 0) throw new IllegalArgumentException("Деление на ноль");
            value = op == '*' ? value * right : value / right;
        }
        return value;
    }

    private double factor() {
        if (at >= expression.length()) throw new IllegalArgumentException("Проверьте выражение");
        char ch = expression.charAt(at);
        if (ch == '+' || ch == '-') {
            at++;
            return (ch == '-' ? -1 : 1) * factor();
        }
        if (ch == '(') {
            at++;
            double value = sum();
            if (at >= expression.length() || expression.charAt(at++) != ')')
                throw new IllegalArgumentException("Проверьте скобки");
            return value;
        }
        int begin = at;
        while (at < expression.length() && (Character.isDigit(expression.charAt(at))
                || expression.charAt(at) == '.')) at++;
        if (begin == at) throw new IllegalArgumentException("Проверьте выражение");
        try { return Double.parseDouble(expression.substring(begin, at)); }
        catch (NumberFormatException error) { throw new IllegalArgumentException("Проверьте выражение"); }
    }
}
