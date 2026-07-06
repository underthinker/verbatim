import React from "react";
import ReactDOM from "react-dom/client";

import Onboarding from "./Onboarding";
import "./onboarding.css";

const root = document.getElementById("root");
if (root !== null) {
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <Onboarding />
    </React.StrictMode>,
  );
}
