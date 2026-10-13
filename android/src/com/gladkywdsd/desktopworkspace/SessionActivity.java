package com.gladkywdsd.desktopworkspace;

import android.app.Activity;
import android.app.AlertDialog;
import android.graphics.Typeface;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.InputType;
import android.view.Gravity;
import android.view.View;
import android.view.WindowManager;
import android.view.inputmethod.EditorInfo;
import android.widget.EditText;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import org.json.JSONObject;

import java.io.IOException;

/** One PC session: its live screen, and a line to type into it. */
public final class SessionActivity extends Activity {
    static final String EXTRA_ID = "id";
    static final String EXTRA_NAME = "name";

    private final Handler main = new Handler(Looper.getMainLooper());
    private Bridge bridge;
    private String id;
    private WebSocket socket;
    private boolean visible;
    private boolean ended;

    private TextView screen;
    private ScrollView vertical;
    private TextView state;
    private EditText input;

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        bridge = Ui.savedBridge(this);
        id = getIntent().getStringExtra(EXTRA_ID);
        if (bridge == null || id == null) {
            finish();
            return;
        }
        getWindow().getDecorView().setBackgroundColor(Ui.BG);
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        int pad = Ui.dp(this, 12);
        root.setPadding(pad, pad + Ui.dp(this, 16), pad, pad);

        LinearLayout header = new LinearLayout(this);
        header.setGravity(Gravity.CENTER_VERTICAL);
        LinearLayout names = new LinearLayout(this);
        names.setOrientation(LinearLayout.VERTICAL);
        TextView title = Ui.text(this, getIntent().getStringExtra(EXTRA_NAME), 19, Ui.TEXT);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        names.addView(title);
        state = Ui.text(this, "Connecting…", 13, Ui.MUTED);
        names.addView(state);
        header.addView(names, new LinearLayout.LayoutParams(0, -2, 1));
        View close = Ui.button(this, "Close", Ui.DANGER);
        close.setOnClickListener(v -> confirmClose());
        header.addView(close, new LinearLayout.LayoutParams(-2, Ui.dp(this, 40)));
        root.addView(header);

        screen = new TextView(this);
        screen.setTypeface(Typeface.MONOSPACE);
        screen.setTextSize(11);
        screen.setTextColor(Ui.TEXT);
        screen.setTextIsSelectable(true);
        screen.setPadding(pad, pad, pad, pad);
        HorizontalScrollView horizontal = new HorizontalScrollView(this);
        horizontal.addView(screen);
        vertical = new ScrollView(this);
        vertical.setBackground(Ui.rounded(Ui.CARD, Ui.dp(this, 10)));
        vertical.addView(horizontal);
        LinearLayout.LayoutParams screenLp = new LinearLayout.LayoutParams(-1, 0, 1);
        screenLp.setMargins(0, Ui.dp(this, 10), 0, Ui.dp(this, 10));
        root.addView(vertical, screenLp);

        LinearLayout keys = new LinearLayout(this);
        String[][] shortcuts = {{"Enter", ""}, {"Yes", "y"}, {"No", "n"}, {"1", "1"}, {"2", "2"}, {"3", "3"}};
        for (String[] key : shortcuts) {
            View b = Ui.button(this, key[0], Ui.CARD);
            ((TextView) b).setTextColor(Ui.TEXT);
            b.setOnClickListener(v -> send(key[1], true));
            LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(0, Ui.dp(this, 40), 1);
            lp.setMargins(0, 0, Ui.dp(this, 6), 0);
            keys.addView(b, lp);
        }
        root.addView(keys);

        LinearLayout line = new LinearLayout(this);
        line.setPadding(0, Ui.dp(this, 8), 0, 0);
        input = new EditText(this);
        input.setHint("Type to the session…");
        input.setHintTextColor(Ui.MUTED);
        input.setTextColor(Ui.TEXT);
        input.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);
        input.setMaxLines(5);
        input.setImeOptions(EditorInfo.IME_ACTION_SEND);
        line.addView(input, new LinearLayout.LayoutParams(0, -2, 1));
        View sendButton = Ui.button(this, "Send", Ui.ACCENT);
        sendButton.setOnClickListener(v -> {
            String text = input.getText().toString();
            if (text.isEmpty()) return;
            input.setText("");
            send(text, true);
        });
        line.addView(sendButton, new LinearLayout.LayoutParams(-2, Ui.dp(this, 48)));
        root.addView(line);
        setContentView(root);
    }

    @Override
    protected void onStart() {
        super.onStart();
        visible = true;
        connect();
    }

    @Override
    protected void onStop() {
        super.onStop();
        visible = false;
        main.removeCallbacksAndMessages(null);
        if (socket != null) socket.close();
        socket = null;
    }

    private void connect() {
        if (!visible || ended || socket != null) return;
        Ui.IO.execute(() -> {
            try {
                WebSocket opened = bridge.openStream(id, new WebSocket.Listener() {
                    @Override
                    public void onText(String text) {
                        runOnUiThread(() -> show(text));
                    }

                    @Override
                    public void onClosed(IOException error) {
                        runOnUiThread(() -> {
                            socket = null;
                            if (ended || !visible) return;
                            state.setText(error == null ? "Reconnecting…" : Ui.explain(error));
                            // The PC closes long streams on purpose: reconnect.
                            main.postDelayed(SessionActivity.this::connect, 1500);
                        });
                    }
                });
                runOnUiThread(() -> {
                    if (!visible) opened.close();
                    else socket = opened;
                });
            } catch (Exception e) {
                runOnUiThread(() -> {
                    state.setText(Ui.explain(e));
                    boolean gone = e instanceof Bridge.BridgeException && ((Bridge.BridgeException) e).status == 404;
                    if (gone) ended = true;
                    else main.postDelayed(this::connect, 3000);
                });
            }
        });
    }

    private void show(String text) {
        JSONObject frame;
        try {
            frame = new JSONObject(text);
        } catch (Exception e) {
            return;
        }
        String status = frame.optString("status");
        String label = frame.optString("label", status);
        String title = frame.optString("sessionTitle");
        state.setText(title.isEmpty() || "null".equals(title) ? label : label + " · " + title);
        state.setTextColor(Ui.statusColor(status));
        String tail = frame.optString("tail", null);
        if (tail != null && !"null".equals(tail)) {
            boolean atBottom = vertical.getScrollY() + vertical.getHeight() >= screen.getHeight() - Ui.dp(this, 40);
            screen.setText(tail.replaceAll("\\s+$", ""));
            if (atBottom) vertical.post(() -> vertical.fullScroll(View.FOCUS_DOWN));
        }
        if ("EXITED".equals(status)) {
            ended = true;
            state.setText("The session has ended.");
        }
    }

    private void send(String text, boolean enter) {
        Ui.IO.execute(() -> {
            try {
                bridge.sendKeys(id, text, enter);
            } catch (Exception e) {
                Ui.toast(this, Ui.explain(e));
            }
        });
    }

    private void confirmClose() {
        new AlertDialog.Builder(this)
                .setMessage("Close this session on the PC? Whatever runs in it is stopped.")
                .setPositiveButton("Close session", (d, w) -> Ui.IO.execute(() -> {
                    try {
                        bridge.close(id);
                        ended = true;
                        runOnUiThread(this::finish);
                    } catch (Exception e) {
                        Ui.toast(this, Ui.explain(e));
                    }
                }))
                .setNegativeButton("Cancel", null)
                .show();
    }
}
