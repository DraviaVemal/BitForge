import { Navigate, Route, Routes } from "react-router-dom";
import Layout from "./Layout";
import Dashboard from "./pages/Dashboard";
import Builds from "./pages/Builds";
import Dependency from "./pages/Dependency";
import RecipeTree from "./pages/RecipeTree";
import Binaries from "./pages/Binaries";
import DiskLayout from "./pages/DiskLayout";
import Environment from "./pages/Environment";
import Cache from "./pages/Cache";

export default function AppRoutes() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<Dashboard />} />
        <Route path="builds" element={<Builds />} />
        <Route path="dependency" element={<Dependency />} />
        <Route path="recipes" element={<RecipeTree />} />
        <Route path="binaries" element={<Binaries />} />
        <Route path="disk" element={<DiskLayout />} />
        <Route path="environment" element={<Environment />} />
        <Route path="cache" element={<Cache />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
