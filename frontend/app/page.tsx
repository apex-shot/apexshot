import { Hero } from "./sections/hero";
import { Features } from "./sections/features";
import { Comparison } from "./sections/comparison";
import { Footer } from "./sections/footer";

export default function Home() {
  return (
    <main className="min-h-screen bg-[#000000]">
      <Hero />
      <Features />
      <Comparison />
      <Footer />
    </main>
  );
}
