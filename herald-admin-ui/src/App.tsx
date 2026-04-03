import { Routes, Route, Navigate } from "react-router-dom";
import { useAuth } from "./context/AuthContext";
import Layout from "./components/Layout";
import Login from "./pages/Login";
import Overview from "./pages/Overview";
import Stats from "./pages/Stats";
import DebugConsole from "./pages/DebugConsole";
import ErrorLogs from "./pages/ErrorLogs";
import Tenants from "./pages/Tenants";
import TenantDetail from "./pages/TenantDetail";
import Rooms from "./pages/Rooms";
import RoomDetail from "./pages/RoomDetail";

export default function App() {
  const { auth } = useAuth();

  if (!auth) return <Login />;

  return (
    <Layout>
      <Routes>
        <Route path="/" element={<Overview />} />
        <Route path="/stats" element={<Stats />} />
        <Route path="/console" element={<DebugConsole />} />
        <Route path="/errors" element={<ErrorLogs />} />
        <Route path="/tenants" element={<Tenants />} />
        <Route path="/tenants/:id" element={<TenantDetail />} />
        <Route path="/rooms" element={<Rooms />} />
        <Route path="/rooms/:id" element={<RoomDetail />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Layout>
  );
}
