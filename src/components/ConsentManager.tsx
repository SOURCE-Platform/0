import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Lock } from "lucide-react";

interface ConsentState {
  screen_recording: boolean;
  os_activity: boolean;
  keyboard_recording: boolean;
  mouse_recording: boolean;
  camera_recording: boolean;
  microphone_recording: boolean;
}

interface FeatureInfo {
  key: keyof ConsentState;
  title: string;
  description: string;
  icon: string;
}

const FEATURES: FeatureInfo[] = [
  {
    key: "screen_recording",
    title: "Screen Recording",
    description: "Capture screenshots and record screen activity for productivity tracking",
    icon: "🖥️",
  },
  {
    key: "os_activity",
    title: "OS Activity Tracking",
    description: "Track application usage and window focus to understand your workflow",
    icon: "📊",
  },
  {
    key: "keyboard_recording",
    title: "Keyboard Recording",
    description: "Record keyboard activity and typing patterns (keystrokes are not logged, only metadata)",
    icon: "⌨️",
  },
  {
    key: "mouse_recording",
    title: "Mouse Recording",
    description: "Track mouse movements and clicks to analyze interaction patterns",
    icon: "🖱️",
  },
  {
    key: "camera_recording",
    title: "Camera Recording",
    description: "Optional: Record video from your camera during sessions",
    icon: "📷",
  },
  {
    key: "microphone_recording",
    title: "Microphone Recording",
    description: "Optional: Record audio from your microphone during sessions",
    icon: "🎤",
  },
];

export default function ConsentManager() {
  const [consents, setConsents] = useState<ConsentState>({
    screen_recording: false,
    os_activity: false,
    keyboard_recording: false,
    mouse_recording: false,
    camera_recording: false,
    microphone_recording: false,
  });
  const [loading, setLoading] = useState(true);
  const [updating, setUpdating] = useState<string | null>(null);

  useEffect(() => {
    loadConsents();
  }, []);

  async function loadConsents() {
    try {
      const allConsents = await invoke<Record<string, boolean>>("get_all_consents");
      // Map backend consent keys to frontend state
      setConsents({
        screen_recording: allConsents.screen_recording || false,
        os_activity: allConsents.os_activity || false,
        keyboard_recording: allConsents.keyboard_recording || false,
        mouse_recording: allConsents.mouse_recording || false,
        camera_recording: allConsents.camera_recording || false,
        microphone_recording: allConsents.microphone_recording || false,
      });
    } catch (error) {
      console.error("Failed to load consents:", error);
    } finally {
      setLoading(false);
    }
  }

  async function toggleConsent(featureKey: keyof ConsentState) {
    setUpdating(featureKey);
    const currentValue = consents[featureKey];

    try {
      if (currentValue) {
        await invoke("revoke_consent", { feature: featureKey });
      } else {
        await invoke("request_consent", { feature: featureKey });
      }

      setConsents((prev) => ({
        ...prev,
        [featureKey]: !currentValue,
      }));
    } catch (error) {
      console.error(`Failed to toggle consent for ${featureKey}:`, error);
    } finally {
      setUpdating(null);
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-[400px]">
        <p className="text-muted-foreground">Loading consent settings...</p>
      </div>
    );
  }

  return (
    <div className="w-full max-w-5xl mx-auto p-6 space-y-6">
      <div className="space-y-2">
        <h1 className="text-3xl font-bold tracking-tight text-foreground">Privacy & Consent Settings</h1>
        <p className="text-muted-foreground">
          Control what data Observer can collect. All features require explicit consent and default to OFF.
        </p>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        {FEATURES.map((feature) => (
          <Card key={feature.key}>
            <CardHeader>
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-3">
                  <span className="text-3xl">{feature.icon}</span>
                  <div>
                    <CardTitle className="text-lg">{feature.title}</CardTitle>
                    <CardDescription className="mt-1.5">
                      {feature.description}
                    </CardDescription>
                  </div>
                </div>
              </div>
            </CardHeader>
            <CardContent>
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <Switch
                    id={feature.key}
                    checked={consents[feature.key]}
                    onCheckedChange={() => toggleConsent(feature.key)}
                    disabled={updating === feature.key}
                  />
                  <Label
                    htmlFor={feature.key}
                    className={`text-sm font-medium cursor-pointer ${
                      updating === feature.key ? "opacity-50" : ""
                    }`}
                  >
                    {updating === feature.key
                      ? "Updating..."
                      : consents[feature.key]
                      ? "Enabled"
                      : "Disabled"}
                  </Label>
                </div>
                <div
                  className={`px-3 py-1 rounded-full text-xs font-medium ${
                    consents[feature.key]
                      ? "bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-100"
                      : "bg-gray-100 text-gray-800 dark:bg-gray-800 dark:text-gray-100"
                  }`}
                >
                  {consents[feature.key] ? "Active" : "Inactive"}
                </div>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      <Card className="border-blue-200 dark:border-blue-900 bg-blue-50 dark:bg-blue-950">
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-blue-900 dark:text-blue-100">
            <Lock className="h-5 w-5" />
            Privacy First
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-2 text-sm text-blue-800 dark:text-blue-200">
          <p>• All data is stored locally on your device</p>
          <p>• No data is sent to external servers</p>
          <p>• You have full control over your data</p>
          <p>• You can revoke consent at any time</p>
        </CardContent>
      </Card>
    </div>
  );
}
