package dev.rustnative.android;

import android.app.job.JobInfo;
import android.app.job.JobParameters;
import android.app.job.JobScheduler;
import android.app.job.JobService;
import android.content.ComponentName;
import android.content.Context;
import android.os.PersistableBundle;

/**
 * Constrained background work (`AndroidWork`): `JobScheduler` runs a job
 * here once its constraints hold — in a process that may have no activity,
 * so the library is loaded and the job's handler (registered with
 * `export_main!(main, work = …)`) runs on a thread of its own.
 */
public class RnJobService extends JobService {
    static final String NAME = "dev.rustnative.job";

    /** Schedules job {@code name}; an error message, or null. */
    static String schedule(int id, String name, boolean network, boolean charging, long deadlineMillis) {
        Context context = RnServices.context();
        PersistableBundle extras = new PersistableBundle();
        extras.putString(NAME, name);
        JobInfo.Builder job = new JobInfo.Builder(id, new ComponentName(context, RnJobService.class))
            .setExtras(extras)
            .setRequiredNetworkType(network ? JobInfo.NETWORK_TYPE_ANY : JobInfo.NETWORK_TYPE_NONE)
            .setRequiresCharging(charging);
        if (deadlineMillis >= 0) {
            job.setOverrideDeadline(deadlineMillis);
        }
        JobScheduler scheduler = context.getSystemService(JobScheduler.class);
        if (scheduler == null) {
            return "no job scheduler";
        }
        return scheduler.schedule(job.build()) == JobScheduler.RESULT_SUCCESS ? null
            : "JobScheduler refused the job (too many pending, or invalid constraints)";
    }

    /** Cancels job {@code id}. */
    static void cancel(int id) {
        JobScheduler scheduler = RnServices.context().getSystemService(JobScheduler.class);
        if (scheduler != null) {
            scheduler.cancel(id);
        }
    }

    /** Whether job {@code id} is waiting to run (tests read it). */
    static boolean pending(int id) {
        JobScheduler scheduler = RnServices.context().getSystemService(JobScheduler.class);
        return scheduler != null && scheduler.getPendingJob(id) != null;
    }

    @Override
    public boolean onStartJob(final JobParameters parameters) {
        RnBridge.load(this);
        final String name = parameters.getExtras().getString(NAME);
        new Thread(new Runnable() {
            @Override
            public void run() {
                boolean again = RnBridge.nativeRunJob(name == null ? "" : name);
                jobFinished(parameters, again);
            }
        }, "rustnative-job").start();
        return true;
    }

    @Override
    public boolean onStopJob(JobParameters parameters) {
        // The constraints stopped holding: run it again once they hold.
        return true;
    }
}
