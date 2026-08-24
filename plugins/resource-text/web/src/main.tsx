import { createRoot } from "react-dom/client";
import { ResourceTextApp } from "./App";
import { readTextFrameContext } from "./text-frame-client";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("Missing Resource Text root element");
createRoot(root).render(<ResourceTextApp frame={readTextFrameContext()} />);
