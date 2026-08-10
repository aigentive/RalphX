import { useEffect, useState } from "react";

import { getClientUpdateChannel, type UpdateChannel } from "@/api/update-channel";

export function useClientUpdateChannel() {
  const [updateChannel, setUpdateChannel] = useState<UpdateChannel>("stable");
  const [isSettled, setIsSettled] = useState(false);
  const [loadError, setLoadError] = useState<unknown>(null);

  useEffect(() => {
    let active = true;
    void getClientUpdateChannel().then(
      (channel) => {
        if (!active) return;
        setUpdateChannel(channel);
        setIsSettled(true);
      },
      (error: unknown) => {
        if (!active) return;
        setLoadError(error);
        setIsSettled(true);
      },
    );
    return () => {
      active = false;
    };
  }, []);

  return {
    updateChannel,
    isSettled,
    isError: loadError !== null,
    loadError,
  };
}
