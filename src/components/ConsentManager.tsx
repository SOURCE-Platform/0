import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./ConsentManager.css";

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

  // Load all consents on mount
  useEffect(() => {
    loadConsents();
  }, []);

  async function loadConsents() {
    try {
      const allConsents = await invoke<Record<string, boolean>>("get_all_consents");
      setConsents(allConsents as ConsentState);
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
        // Revoke consent
        await invoke("revoke_consent", { feature: featureKey });
      } else {
        // Grant consent
        await invoke("request_consent", { feature: featureKey });
      }

      // Update local state
      setConsents((prev) => ({
        ...prev,
        [featureKey]: !currentValue,
      }));
    } catch (error) {
      console.error(`Failed to toggle consent for ${featureKey}:`, error);
      alert(`Failed to update consent: ${error}`);
    } finally {
      setUpdating(null);
    }
  }

  if (loading) {
    return (
      <div className="consent-manager loading">
        <p>Loading consent settings...</p>
      </div>
    );
  }

  return (
    <div className="consent-manager">
      <div className="consent-header">
        <h1>Privacy & Consent Settings</h1>
        <p className="subtitle">
          Control what data Observer can collect. All features require explicit consent and default to OFF.
        </p>
      </div>

      <div className="consent-features">
        {FEATURES.map((feature) => (
          <div key={feature.key} className="feature-card">
            <div className="feature-info">
              <div className="feature-icon">{feature.icon}</div>
              <div className="feature-content">
                <h3>{feature.title}</h3>
                <p>{feature.description}</p>
              </div>
            </div>
            <div className="feature-control">
              <label className="toggle-switch">
                <input
                  type="checkbox"
                  checked={consents[feature.key]}
                  onChange={() => toggleConsent(feature.key)}
                  disabled={updating === feature.key}
                />
                <span className="toggle-slider"></span>
              </label>
              <span className={`status ${consents[feature.key] ? "enabled" : "disabled"}`}>
                {updating === feature.key ? "Updating..." : consents[feature.key] ? "Enabled" : "Disabled"}
              </span>
            </div>
          </div>
        ))}
      </div>

      <div className="consent-footer">
        <p className="privacy-note">
          🔒 All data is stored locally on your device. You can revoke consent at any time.
        </p>
      </div>
    </div>
  );
}
