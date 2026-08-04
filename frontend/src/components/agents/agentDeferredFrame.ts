import { useCallback, useEffect, useRef, useState } from "react";

export type DeferredFrameJob = {
  frame: number | null;
  timer: number | null;
  cancelFrame: (frame: number) => void;
  cancelTimer: (timer: number) => void;
};

export function cancelDeferredFrameJob(job: DeferredFrameJob | null) {
  if (!job) {
    return;
  }
  if (job.frame !== null) {
    job.cancelFrame(job.frame);
  }
  if (job.timer !== null) {
    job.cancelTimer(job.timer);
  }
}

export function scheduleDeferredFrameJob(callback: () => void): DeferredFrameJob {
  const job: DeferredFrameJob = {
    frame: null,
    timer: null,
    cancelFrame: window.cancelAnimationFrame.bind(window),
    cancelTimer: window.clearTimeout.bind(window),
  };
  job.frame = window.requestAnimationFrame(() => {
    job.frame = null;
    job.timer = window.setTimeout(() => {
      job.timer = null;
      callback();
    }, 0);
  });
  return job;
}

export function useAfterPaintMounted(isVisible: boolean) {
  const [isMounted, setIsMounted] = useState(false);
  const jobRef = useRef<DeferredFrameJob | null>(null);

  const cancelJob = useCallback(() => {
    cancelDeferredFrameJob(jobRef.current);
    jobRef.current = null;
  }, []);

  useEffect(() => () => cancelJob(), [cancelJob]);

  useEffect(() => {
    cancelJob();
    if (isVisible) {
      if (!isMounted) {
        jobRef.current = scheduleDeferredFrameJob(() => {
          jobRef.current = null;
          setIsMounted(true);
        });
      }
      return;
    }

    if (isMounted) {
      jobRef.current = scheduleDeferredFrameJob(() => {
        jobRef.current = null;
        setIsMounted(false);
      });
    }
  }, [cancelJob, isMounted, isVisible]);

  return isMounted;
}
