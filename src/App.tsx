import { HashRouter } from "react-router-dom";
import { AppRoutes } from "./routes";
import "./App.css";
import MainLayout from "./layouts/MainLayout";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { PlayerProvider } from "./context/PlayerContext";

const queryClient = new QueryClient();

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <PlayerProvider>
        <HashRouter>
          <MainLayout>
            <AppRoutes />
          </MainLayout>
        </HashRouter>
      </PlayerProvider>
    </QueryClientProvider>
  );
}

export default App;
