// src/components/Layout.tsx
import React from "react";
import { BottomPlayerBar } from "../components/BottomPlayerBar";
import HeaderBar from "../components/HeaderBar";
import { Sidebar } from "../components/SideBar";

interface LayoutProps {
  children: React.ReactNode;
}

const Layout: React.FC<LayoutProps> = ({ children }) => {
  return (
    <div className="flex h-screen overflow-hidden bg-[#0d1117] text-[#c9d1d9]">
      {/* Sidebar Navigation */}
      <Sidebar />

      {/* Main Content Area */}
      <div className="flex flex-1 flex-col overflow-hidden">
        <HeaderBar />

        <main className="flex-1 overflow-y-auto bg-gradient-to-b from-[#161b22] to-[#0d1117] px-6 pb-28 pt-4">
          {children}
        </main>
      </div>

      {/* Persistent Player */}
      <BottomPlayerBar />
    </div>
  );
};

export default Layout;
