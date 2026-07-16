import React from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import { AuthGate } from "./components/AuthGate";
import { ResourceWorkspace } from "./features/ResourceWorkspace";
import { StandalonePluginView, readStandalonePluginTarget } from "./features/StandalonePluginView";

const standalonePluginTarget = readStandalonePluginTarget();

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <AuthGate>
      {({ user, initialDirectory, logout }) => (
        standalonePluginTarget
          ? <StandalonePluginView target={standalonePluginTarget} />
          : <ResourceWorkspace user={user} initialDirectory={initialDirectory} onLogout={logout} />
      )}
    </AuthGate>
  </React.StrictMode>,
);
