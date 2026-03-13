import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import { Navigation } from "@/components/navigation";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "ApexShot — Premium Screen Capture for Linux",
  description:
    "The CleanShot X experience you've been waiting for. Capture, annotate, and share with ease. Built specifically for Linux enthusiasts.",
  keywords: [
    "ApexShot",
    "screen capture",
    "Linux",
    "screenshot tool",
    "screen recording",
    "annotation",
    "OCR",
  ],
  authors: [{ name: "ApexShot Team" }],
  openGraph: {
    title: "ApexShot — Premium Screen Capture for Linux",
    description:
      "The CleanShot X experience you've been waiting for. Capture, annotate, and share with ease.",
    type: "website",
    locale: "en_US",
  },
  twitter: {
    card: "summary_large_image",
    title: "ApexShot — Premium Screen Capture for Linux",
    description:
      "The CleanShot X experience you've been waiting for. Capture, annotate, and share with ease.",
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className="dark">
      <body
        className={`${geistSans.variable} ${geistMono.variable} antialiased bg-neutral-950 text-white`}
      >
        <Navigation />
        {children}
      </body>
    </html>
  );
}
