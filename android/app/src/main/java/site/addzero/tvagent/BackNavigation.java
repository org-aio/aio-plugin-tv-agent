package site.addzero.tvagent;

import android.annotation.TargetApi;
import android.app.Activity;
import android.os.Build;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;

final class BackNavigation {
    private BackNavigation() {
    }

    static Object register(Activity activity, Runnable action) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            return null;
        }
        return Api33.register(activity, action);
    }

    static void unregister(Activity activity, Object registration) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || registration == null) {
            return;
        }
        Api33.unregister(activity, registration);
    }

    @TargetApi(Build.VERSION_CODES.TIRAMISU)
    private static final class Api33 {
        private Api33() {
        }

        static Object register(Activity activity, Runnable action) {
            OnBackInvokedCallback callback = action::run;
            activity.getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                OnBackInvokedDispatcher.PRIORITY_DEFAULT,
                callback
            );
            return callback;
        }

        static void unregister(Activity activity, Object registration) {
            activity.getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback(
                (OnBackInvokedCallback) registration
            );
        }
    }
}
