package app.valanium;

import android.animation.ValueAnimator;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.view.View;

/** Local animation only; does not enable or transmit activity signals. */
public final class TypingIndicator extends View {
    private final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
    private ValueAnimator animator;
    private float phase;
    public TypingIndicator(Context context,android.util.AttributeSet attrs){super(context,attrs);}
    public void setColor(int color){paint.setColor(color);invalidate();}
    private void update() {
        if(getWindowVisibility()==VISIBLE&&isShown()&&isAttachedToWindow()&&ValueAnimator.areAnimatorsEnabled()) {
            if(animator!=null)return;
            animator=ValueAnimator.ofFloat(0,1);animator.setInterpolator(new android.view.animation.LinearInterpolator());animator.setDuration(1200);animator.setRepeatCount(ValueAnimator.INFINITE);
            animator.addUpdateListener(a->{phase=(float)a.getAnimatedValue();invalidate();});animator.start();
        } else if(animator!=null){animator.cancel();animator=null;}
    }
    @Override protected void onWindowVisibilityChanged(int visibility){super.onWindowVisibilityChanged(visibility);update();}
    @Override protected void onAttachedToWindow(){super.onAttachedToWindow();update();}
    @Override protected void onDetachedFromWindow(){if(animator!=null){animator.cancel();animator=null;}super.onDetachedFromWindow();}
    @Override protected void onVisibilityChanged(View changed,int visibility){super.onVisibilityChanged(changed,visibility);update();}
    @Override protected void onDraw(Canvas canvas){super.onDraw(canvas);float d=getResources().getDisplayMetrics().density;
        for(int i=0;i<3;i++){float p=(phase-i*.15f+1)%1;float pulse=Math.max(0,1-Math.abs(p-.25f)*4);paint.setAlpha((int)(110+145*pulse));canvas.drawCircle((5+i*10)*d,getHeight()/2f,(2+1.4f*pulse)*d,paint);}}
}
