package app.valanium;

import android.graphics.Color;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.LayerDrawable;
import android.graphics.drawable.StateListDrawable;
import android.view.View;
import android.view.ViewGroup;
import android.widget.TextView;
import android.widget.ImageView;
import android.content.res.ColorStateList;

/** Rebuilds legacy violet resource surfaces from their original drawable state. */
final class ThemeSurfaces {
    static boolean violet(int color) {
        float[] hsv=new float[3];Color.colorToHSV(color,hsv);
        return Color.alpha(color)>0 && hsv[1]>.12f && hsv[0]>=245 && hsv[0]<=295;
    }
    static int tone(int original,int accent,boolean light) {
        if(!violet(original))return original;
        float[] hsv=new float[3];Color.colorToHSV(original,hsv);
        if(hsv[2]>.48f)return Color.argb(Color.alpha(original),Color.red(accent),Color.green(accent),Color.blue(accent));
        int base=light?248:20;
        return Color.argb(Color.alpha(original),(base*92+Color.red(accent)*8)/100,
            (base*92+Color.green(accent)*8)/100,(base*92+Color.blue(accent)*8)/100);
    }
    private static void drawable(Drawable value,int accent,boolean light,float density) {
        if(value instanceof GradientDrawable) {
            GradientDrawable shape=(GradientDrawable)value;
            int[] colors=shape.getColors();boolean changed=false;
            if(colors!=null) {
                for(int i=0;i<colors.length;i++){int next=tone(colors[i],accent,light);changed|=next!=colors[i];colors[i]=next;}
                shape.setColors(colors);
            } else if(shape.getColor()!=null) {
                int before=shape.getColor().getDefaultColor(),next=tone(before,accent,light);
                changed=before!=next;if(changed)shape.setColor(next);
            }
            if(changed)shape.setStroke(Math.max(1,Math.round(density)),Color.argb(65,Color.red(accent),Color.green(accent),Color.blue(accent)));
        } else if(value instanceof LayerDrawable) {
            LayerDrawable layers=(LayerDrawable)value;
            for(int i=0;i<layers.getNumberOfLayers();i++)drawable(layers.getDrawable(i),accent,light,density);
        } else if(value instanceof StateListDrawable) {
            StateListDrawable states=(StateListDrawable)value;
            if(android.os.Build.VERSION.SDK_INT>=29)for(int i=0;i<states.getStateCount();i++)drawable(states.getStateDrawable(i),accent,light,density);
            else drawable(states.getCurrent(),accent,light,density);
        }
    }
    static void apply(View view,int accent,boolean light) {
        int id=view.getId();
        // Profile colors and semantic recording/network colors belong to their data.
        if(id==R.id.accent_violet||id==R.id.accent_white||id==R.id.accent_blue||id==R.id.accent_green||id==R.id.accent_coral||id==R.id.profile_avatar||id==R.id.peer_avatar||id==R.id.chat_logo||id==R.id.recording_bar||id==R.id.status||id==R.id.status_dot)return;
        Drawable background=view.getBackground();
        if(background!=null) {
            Object original=view.getTag(R.id.theme_drawable_tag);
            if(original==null&&background.getConstantState()!=null){original=background.getConstantState();view.setTag(R.id.theme_drawable_tag,original);}
            if(original instanceof Drawable.ConstantState) {
                Drawable copy=((Drawable.ConstantState)original).newDrawable(view.getResources()).mutate();
                drawable(copy,accent,light,view.getResources().getDisplayMetrics().density);
                if(id!=R.id.app_root&&!"valanium_top_bar".equals(view.getTag()))view.setBackground(copy);
            }
        }
        if(view instanceof ImageView) {
            ImageView image=(ImageView)view;Object original=image.getTag(R.id.theme_icon_tag);
            if(original==null&&image.getImageTintList()!=null){original=image.getImageTintList().getDefaultColor();image.setTag(R.id.theme_icon_tag,original);}
            if(original instanceof Integer&&((Integer)original==view.getContext().getColor(R.color.valanium_icon)||violet((Integer)original)))image.setImageTintList(ColorStateList.valueOf(accent));
        }
        if(view instanceof android.widget.Switch){android.widget.Switch toggle=(android.widget.Switch)view;toggle.setThumbTintList(new ColorStateList(new int[][]{new int[]{android.R.attr.state_checked},new int[]{}},new int[]{accent,light?Color.GRAY:Color.LTGRAY}));toggle.setTrackTintList(new ColorStateList(new int[][]{new int[]{android.R.attr.state_checked},new int[]{}},new int[]{Color.argb(100,Color.red(accent),Color.green(accent),Color.blue(accent)),light?0xffdddddf:0xff444447}));}
        if(view instanceof android.widget.SeekBar){android.widget.SeekBar bar=(android.widget.SeekBar)view;bar.setProgressTintList(ColorStateList.valueOf(accent));bar.setThumbTintList(ColorStateList.valueOf(accent));}
        if(view instanceof TextView) {
            TextView text=(TextView)view;Object original=text.getTag(R.id.base_text_color_tag);
            int color=original instanceof Integer?(Integer)original:text.getCurrentTextColor();
            if(violet(color)){text.setTag(R.id.base_text_color_tag,color);text.setTextColor(accent);}
        }
        if(view instanceof ViewGroup)for(int i=0;i<((ViewGroup)view).getChildCount();i++)apply(((ViewGroup)view).getChildAt(i),accent,light);
    }
}
