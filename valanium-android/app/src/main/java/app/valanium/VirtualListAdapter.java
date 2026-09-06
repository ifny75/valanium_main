package app.valanium;

import android.view.View;
import android.view.ViewGroup;
import android.widget.BaseAdapter;

import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.List;

/**
 * Небольшой системный адаптер без AndroidX.
 *
 * Он хранит только модели строк, а View создаёт для видимой области и отдаёт
 * старую строку renderer-у при прокрутке. Так длинные ленты не остаются одним
 * огромным деревом View и не требуют новой внешней зависимости.
 */
final class VirtualListAdapter<T> extends BaseAdapter {
    interface Renderer<T> {
        int viewTypeCount();
        int viewType(T item, int position);
        View render(T item, int position, View recycled, ViewGroup parent);
    }

    private final Renderer<T> renderer;
    private List<T> items = Collections.emptyList();

    VirtualListAdapter(Renderer<T> renderer) {
        this.renderer = renderer;
    }

    void submit(Collection<T> next) {
        items = Collections.unmodifiableList(new ArrayList<>(next));
        notifyDataSetChanged();
    }

    @Override public int getCount() {
        return items.size();
    }

    @Override public T getItem(int position) {
        return items.get(position);
    }

    @Override public long getItemId(int position) {
        return position;
    }

    @Override public int getViewTypeCount() {
        return Math.max(1, renderer.viewTypeCount());
    }

    @Override public int getItemViewType(int position) {
        return renderer.viewType(getItem(position), position);
    }

    @Override public boolean hasStableIds() {
        return false;
    }

    @Override public boolean isEmpty() {
        return items.isEmpty();
    }

    @Override public View getView(int position, View convertView, ViewGroup parent) {
        return renderer.render(getItem(position), position, convertView, parent);
    }
}
