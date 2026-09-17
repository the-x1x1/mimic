import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { toast } from "@/state/toast";

export function useLightroomStatus() {
  return useQuery({ queryKey: qk.lightroom, queryFn: ipc.lightroomStatus, refetchInterval: 4_000 });
}

export function usePluginSetup() {
  return useQuery({ queryKey: qk.pluginSetup, queryFn: ipc.pluginSetup, staleTime: Infinity });
}

export function useCapabilityMatrix() {
  return useQuery({ queryKey: qk.capabilities, queryFn: ipc.capabilityMatrix });
}

export function useInstallPlugin() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.installPlugin,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.lightroom });
      toast.success("Plugin files are ready", "Add the folder in Lightroom's Plug-in Manager.");
    },
    onError: (e: Error) => toast.danger("Plugin install failed", e.message),
  });
}

export function useTestLightroom() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ipc.testLightroom,
    onSuccess: (r) => {
      qc.invalidateQueries({ queryKey: qk.lightroom });
      qc.invalidateQueries({ queryKey: qk.capabilities });
      if (r.connected)
        toast.success("Lightroom responded", `${r.roundTripMs ?? "?"} ms round trip`);
      else toast.warning("Lightroom not connected", r.message);
    },
  });
}

export function useStartLightroomIngest() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (args: { libraryId?: string; name?: string; scope?: string }) =>
      ipc.startLightroomIngest(args),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: qk.jobs });
      qc.invalidateQueries({ queryKey: qk.libraries });
      toast.info("Capturing develop settings from Lightroom");
    },
    onError: (e: Error) => toast.danger("Could not start Lightroom capture", e.message),
  });
}
