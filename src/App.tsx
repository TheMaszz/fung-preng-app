import { HashRouter } from "react-router-dom";
import { AppRoutes } from "./routes";
import "./App.css";
import MainLayout from "./layouts/MainLayout";

function App() {
  return (
    <HashRouter>
      <MainLayout>
        <AppRoutes />
      </MainLayout>
    </HashRouter>
  );
}

export default App;
