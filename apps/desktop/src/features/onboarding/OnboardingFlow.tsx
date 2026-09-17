import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";
import { Button, InlineError, Metric } from "@mimic/ui";
import { ArrowRight, Cable, FolderOpen, FlaskConical } from "lucide-react";
import { LightroomSetup } from "@/components/LightroomSetup";
import { DataQualityPanel } from "@/components/DataQualityPanel";
import { ipc } from "@/lib/ipc";
import { qk } from "@/app/queryClient";
import { useCreateLibrary, useDataQualityReport, useStartScan } from "@/hooks/useLibraries";
import { useCreateStyle, useStyles, useTrainStyle } from "@/hooks/useStyles";
import { primaryError } from "@mimic/contracts";
import { useLightroomStatus, useStartLightroomIngest } from "@/hooks/useLightroom";
import { useJobs } from "@/hooks/useJobs";
import { useNativeEventBridge, useSystemStatus } from "@/hooks/useSystem";
import { Toaster } from "@/components/Toaster";
import { JOB_LABELS } from "@mimic/contracts";
import { ProgressBar } from "@mimic/ui";

type Source = "lightroom" | "folder" | "demo";
type Step = "source" | "lightroom" | "style" | "quality" | "train";

export function OnboardingFlow() {
  useNativeEventBridge();
  const navigate = useNavigate();
  const qc = useQueryClient();
  const [step, setStep] = useState<Step>("source");
  const [source, setSource] = useState<Source>("folder");
  const [name, setName] = useState("");
  const [folder, setFolder] = useState<string | null>(null);
  const [libraryId, setLibraryId] = useState<string | null>(null);
  const [styleId, setStyleId] = useState<string | null>(null);
  const train = useTrainStyle();
  const stylesQ = useStyles();
  const createdStyle = stylesQ.data?.find((s) => s.id === styleId);
  const [error, setError] = useState<string | null>(null);
  const createLibrary = useCreateLibrary();
  const createStyle = useCreateStyle();
  const scan = useStartScan();
  const ingest = useStartLightroomIngest();
  const lr = useLightroomStatus();
  const system = useSystemStatus();
  const report = useDataQualityReport(libraryId);
  const jobs = useJobs(true);
  const trainingJob = jobs.data?.find(
    (j) =>
      j.type === "train_style" && (j.payload as { styleId?: string } | null)?.styleId === styleId,
  );
  const activeJob = jobs.data?.find(
    (j) => (j.payload as { libraryId?: string } | null)?.libraryId === libraryId,
  );

  const finish = async () => {
    await ipc.completeOnboarding();
    await qc.invalidateQueries({ queryKey: qk.onboarding });
    navigate("/styles");
  };

  const chooseSource = async (s: Source) => {
    setSource(s);
    setError(null);
    if (s === "demo") {
      try {
        const lib = await ipc.enableDemoMode();
        setLibraryId(lib.id);
        const st = await createStyle.mutateAsync({
          name: "DEMO Style",
          description: "Synthetic sample data — not a real editing style.",
          libraryId: lib.id,
        });
        setStyleId(st.id);
        setStep("quality");
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
      return;
    }
    setStep(s === "lightroom" ? "lightroom" : "style");
  };

  const createAndScan = async () => {
    setError(null);
    if (!name.trim()) return setError("Name your first Style.");
    if (system.data?.engine.state !== "ready")
      return setError(
        "The analysis engine is still starting. Give it a moment, or check Settings › Diagnostics.",
      );
    try {
      if (source === "folder") {
        if (!folder) return setError("Choose the folder with your edited RAW files.");
        const lib = await createLibrary.mutateAsync({
          name: `${name.trim()} — folder`,
          sourceType: "folder_sidecars",
          rootPath: folder,
        });
        const st = await createStyle.mutateAsync({
          name: name.trim(),
          description: null,
          libraryId: lib.id,
        });
        setStyleId(st.id);
        await scan.mutateAsync(lib.id);
        setLibraryId(lib.id);
      } else {
        if (!lr.data?.bridge.connected) return setError("Lightroom is not connected yet.");
        const lib = await createLibrary.mutateAsync({
          name: `${name.trim()} — Lightroom`,
          sourceType: "lightroom_catalog",
          rootPath: null,
        });
        const st = await createStyle.mutateAsync({
          name: name.trim(),
          description: null,
          libraryId: lib.id,
        });
        setStyleId(st.id);
        await ingest.mutateAsync({ libraryId: lib.id, scope: "selection" });
        setLibraryId(lib.id);
      }
      setStep("quality");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="onboarding">
      <div className="onboarding__card">
        <div className="brand brand--large">
          <span className="brand__mark" aria-hidden />
          <span className="brand__name">Mimic</span>
        </div>

        {step === "source" ? (
          <>
            <h1>Teach Mimic how you edit.</h1>
            <p className="muted">
              Mimic learns from photos you have already edited in Lightroom Classic and reproduces
              your global develop decisions on new shoots. Everything stays on this computer.
            </p>
            <div className="choice-grid">
              <button className="choice" onClick={() => chooseSource("lightroom")}>
                <Cable size={22} />
                <strong>Connect Lightroom Classic</strong>
                <span>Best data: live develop settings straight from your catalog.</span>
              </button>
              <button className="choice" onClick={() => chooseSource("folder")}>
                <FolderOpen size={22} />
                <strong>Train from folders + sidecars</strong>
                <span>Point at RAW files with .xmp sidecars. Read-only.</span>
              </button>
              <button className="choice choice--muted" onClick={() => chooseSource("demo")}>
                <FlaskConical size={22} />
                <strong>Explore with sample data</strong>
                <span>
                  Clearly labelled synthetic DEMO images. Never confused with a live connection.
                </span>
              </button>
            </div>
            {error ? <InlineError>{error}</InlineError> : null}
          </>
        ) : null}

        {step === "lightroom" ? (
          <>
            <h1>Set up the Lightroom plugin</h1>
            <LightroomSetup compact />
            <div className="row gap-2 end mt-4">
              <Button onClick={() => setStep("source")}>Back</Button>
              <Button
                variant="primary"
                icon={<ArrowRight />}
                onClick={() => setStep("style")}
                disabled={!lr.data?.bridge.connected}
                title={
                  lr.data?.bridge.connected ? "" : "Connected appears only after a real handshake"
                }
              >
                Continue
              </Button>
            </div>
          </>
        ) : null}

        {step === "style" ? (
          <>
            <h1>Create your first Style</h1>
            <p className="muted">One Style per editing intent. You can add more sources later.</p>
            <div className="stack gap-3">
              <input
                className="input"
                placeholder="Wedding Natural"
                value={name}
                onChange={(e) => setName(e.target.value)}
                autoFocus
              />
              {source === "folder" ? (
                <div className="path-row">
                  <code className="path">{folder ?? "No folder chosen"}</code>
                  <Button
                    icon={<FolderOpen />}
                    onClick={async () =>
                      setFolder(
                        (await ipc.pickFolder("Choose a folder of edited photos")) ?? folder,
                      )
                    }
                  >
                    Choose folder…
                  </Button>
                </div>
              ) : (
                <p className="muted small">
                  Mimic will capture the current Lightroom selection. Select the photos you want to
                  learn from before continuing.
                </p>
              )}
              {error ? <InlineError>{error}</InlineError> : null}
              <div className="row gap-2 end">
                <Button onClick={() => setStep("source")}>Back</Button>
                <Button
                  variant="primary"
                  icon={<ArrowRight />}
                  onClick={createAndScan}
                  loading={
                    createLibrary.isPending ||
                    createStyle.isPending ||
                    scan.isPending ||
                    ingest.isPending
                  }
                >
                  Scan
                </Button>
              </div>
            </div>
          </>
        ) : null}

        {step === "quality" ? (
          <>
            <h1>{activeJob ? "Scanning…" : "Data quality"}</h1>
            {activeJob ? (
              <div className="stack gap-2">
                <ProgressBar
                  current={activeJob.progressCurrent}
                  total={activeJob.progressTotal}
                  label={`${JOB_LABELS[activeJob.type] ?? activeJob.type} · ${activeJob.phase ?? "starting"}`}
                />
                <p className="muted small">
                  Progress counts photos, not time. You can leave this screen; scanning continues in
                  the background.
                </p>
              </div>
            ) : null}
            {report.data ? <DataQualityPanel report={report.data} /> : null}
            {source === "demo" ? (
              <InlineError title="DEMO data">
                These are synthetic images with sample settings, for exploring the interface only.
              </InlineError>
            ) : null}
            <div className="row gap-2 end mt-4">
              <Button onClick={finish}>Finish without training</Button>
              <Button
                variant="primary"
                icon={<ArrowRight />}
                disabled={
                  !!activeJob ||
                  !report.data ||
                  report.data.recommendation.level === "insufficient" ||
                  !styleId
                }
                title={
                  report.data?.recommendation.level === "insufficient"
                    ? "Not enough edited examples to train"
                    : ""
                }
                onClick={() => {
                  if (styleId) train.mutate({ styleId }, { onSuccess: () => setStep("train") });
                }}
                loading={train.isPending}
              >
                Train
              </Button>
            </div>
          </>
        ) : null}

        {step === "train" ? (
          <>
            <h1>
              {trainingJob
                ? "Training your Style Brain…"
                : createdStyle?.activeVersion
                  ? "Your first version is ready"
                  : "Training"}
            </h1>
            {trainingJob ? (
              <div className="stack gap-2">
                <ProgressBar
                  current={trainingJob.progressCurrent}
                  total={trainingJob.progressTotal}
                  label={trainingJob.phase ?? "starting"}
                />
                <p className="muted small">
                  Progress follows the trainer's phases (loading pairs, splitting by shoot, fitting,
                  evaluating, writing artifacts) — not a clock.
                </p>
              </div>
            ) : createdStyle?.activeVersion ? (
              <div className="metric-grid">
                <Metric label="Version" value={`v${createdStyle.activeVersion.semanticVersion}`} />
                <Metric
                  label="Holdout error (nMAE)"
                  value={primaryError(createdStyle.activeVersion.metrics)?.toFixed(4) ?? "—"}
                  hint="measured on shoots the model never saw"
                />
                <Metric label="Examples" value={createdStyle.trainingExamples.toLocaleString()} />
              </div>
            ) : (
              <InlineError title="Training did not produce an active version">
                Open the Style's Versions tab for the failure reason.
              </InlineError>
            )}
            <div className="row gap-2 end mt-4">
              <Button variant="primary" onClick={finish} disabled={!!trainingJob}>
                Finish
              </Button>
            </div>
          </>
        ) : null}
      </div>
      <Toaster />
    </div>
  );
}
