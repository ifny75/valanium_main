package app.valanium;

import org.junit.Test;
import static org.junit.Assert.assertEquals;

public class CalculatorTest {
    @Test public void suggestsTrailingExpressionLikeDesktop() {
        assertEquals("=14", Calculator.suggestion("Цена: 2*(3+4)"));
        assertEquals("14", Calculator.suggestion("2*(3+4)="));
        assertEquals("=3,5", Calculator.suggestion("1,5+2"));
        assertEquals("=7", Calculator.suggestion("первая строка\n3+4"));
    }

    @Test public void supportsDesktopFunctionsAndNesting() {
        assertEquals("=5", Calculator.suggestion("max(sqrt(16);abs(-5))"));
        assertEquals("=2", Calculator.suggestion("min(3;log(100);4)"));
        assertEquals("=4", Calculator.suggestion("SQRT(16)"));
    }

    @Test public void hidesIncompleteInvalidAndPlainMessages() {
        for (String text : new String[]{"Привет", "123", "2+", "1/0", "sqrt(-1)",
                "max(1)", "2+3 текст", "2+3\n", "foo(2)", "2**3"}) {
            assertEquals(text, "", Calculator.suggestion(text));
        }
    }

    @Test public void handlesPrecedenceAndRounding() {
        assertEquals("10", Calculator.evaluate("2+2*4"));
        assertEquals("0.3", Calculator.evaluate("0.1+0.2"));
        assertEquals("-6", Calculator.evaluate("2*-3"));
    }
}
