package app.valanium;

import java.math.BigDecimal;
import java.math.MathContext;

/** Small arithmetic parser for the message composer; never evaluates executable code. */
final class Calculator {
    private final String expression;
    private int at;

    private Calculator(String source) {
        expression = source.toLowerCase(java.util.Locale.ROOT).replace(',', '.').replaceAll("\\s+", "");
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

    /** Same trailing-expression detection as the desktop composer. */
    static String suggestion(String source) {
        String line = source.substring(source.lastIndexOf('\n') + 1).replaceFirst("\\s+$", "");
        java.util.regex.Matcher match = java.util.regex.Pattern.compile(
                "(?:^|[\\s:])((?:\\d|\\(|(?:max|min|log|sqrt|abs)\\()[a-z\\d\\s.,()+\\-*/;]*)(=?)$",
                java.util.regex.Pattern.CASE_INSENSITIVE).matcher(line);
        if (!match.find()) return "";
        String expression = match.group(1).trim();
        if (!java.util.regex.Pattern.compile("(?:\\d|\\))\\s*[+\\-*/]\\s*(?:\\d|\\()")
                .matcher(expression).find()
                && !expression.matches("(?i)^(?:max|min|log|sqrt|abs)\\(.*")) return "";
        try {
            return (match.group(2).isEmpty() ? "=" : "") + evaluate(expression).replace('.', ',');
        } catch (IllegalArgumentException error) {
            return "";
        }
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
        if (ch >= 'a' && ch <= 'z') {
            int start = at;
            while (at < expression.length() && expression.charAt(at) >= 'a'
                    && expression.charAt(at) <= 'z') at++;
            String name = expression.substring(start, at);
            if (at >= expression.length() || expression.charAt(at++) != '(')
                throw new IllegalArgumentException("Проверьте функцию");
            java.util.ArrayList<Double> args = new java.util.ArrayList<>();
            args.add(sum());
            while (at < expression.length() && expression.charAt(at) == ';') {
                at++;
                args.add(sum());
            }
            if (at >= expression.length() || expression.charAt(at++) != ')')
                throw new IllegalArgumentException("Проверьте скобки");
            if ((name.equals("max") || name.equals("min")) && args.size() >= 2) {
                double value = args.get(0);
                for (int i = 1; i < args.size(); i++) value = name.equals("max")
                        ? Math.max(value, args.get(i)) : Math.min(value, args.get(i));
                return value;
            }
            if (args.size() == 1) {
                double value = args.get(0);
                if (name.equals("abs")) return Math.abs(value);
                if (name.equals("sqrt")) return Math.sqrt(value);
                if (name.equals("log")) return Math.log10(value);
            }
            throw new IllegalArgumentException("Проверьте функцию");
        }
        int begin = at;
        while (at < expression.length() && (Character.isDigit(expression.charAt(at))
                || expression.charAt(at) == '.')) at++;
        if (begin == at) throw new IllegalArgumentException("Проверьте выражение");
        try { return Double.parseDouble(expression.substring(begin, at)); }
        catch (NumberFormatException error) { throw new IllegalArgumentException("Проверьте выражение"); }
    }
}
