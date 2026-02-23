"use client";

import Link from "next/link";
import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Menu, X } from "lucide-react";

const navLinks = [
  { href: "#features", label: "Features" },
  { href: "/pricing", label: "Pricing" },
  { href: "/contact", label: "Contact" },
  { href: "#docs", label: "Documentation" },
];

export function Navigation() {
  const [isOpen, setIsOpen] = useState(false);

  return (
    <header className="fixed top-0 left-0 right-0 z-50">
      <nav className="mx-auto max-w-7xl px-4 sm:px-6 lg:px-8">
        <div className="mt-4 flex items-center justify-between">
          {/* Logo */}
          <Link href="/" className="flex items-center gap-3 group">
            {/* Logo Mark: Sleek Apex + Focus Line */}
            <div className="relative flex h-[34px] w-[34px] items-center justify-center rounded-[10px] bg-gradient-to-b from-[#18181b] to-[#09090b] shadow-[inset_0_1px_rgba(255,255,255,0.1),_0_2px_8px_rgba(0,0,0,0.8)] border border-white/[0.05] group-hover:scale-105 transition-transform duration-300">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="url(#apex-gradient)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="relative z-10">
                <defs>
                  <linearGradient id="apex-gradient" x1="0%" y1="0%" x2="100%" y2="100%">
                    <stop offset="0%" stopColor="#ffffff" />
                    <stop offset="100%" stopColor="#a1a1aa" />
                  </linearGradient>
                </defs>
                {/* The "A" / Peak */}
                <path d="M4 22L12 4L20 22" />
                {/* The focus/capture horizontal dashed line */}
                <path d="M2 15H22" strokeDasharray="3 4" strokeWidth="1.5" />
                {/* The peak dot */}
                <circle cx="12" cy="4" r="1.5" fill="#ffffff" stroke="none" />
              </svg>
            </div>

            {/* Logo Text: Clean, tightly kerned typography */}
            <span className="text-[19px] tracking-[-0.03em] font-medium text-[#F4F4F5] group-hover:text-white transition-colors">
              Apex<span className="text-[#A1A1AA] font-light">s h o t</span>
            </span>
          </Link>

          {/* Desktop Navigation - Pill Style */}
          <div className="hidden md:flex items-center">
            <div className="flex items-center gap-1 rounded-full bg-neutral-900/80 backdrop-blur-md px-2 py-1.5 border border-neutral-800">
              {navLinks.map((link) => (
                <Link
                  key={link.href}
                  href={link.href}
                  className="px-4 py-1.5 text-sm text-neutral-300 hover:text-white transition-colors rounded-full hover:bg-neutral-800"
                >
                  {link.label}
                </Link>
              ))}
            </div>
          </div>

          {/* CTA Button */}
          <div className="hidden md:block">
            <Link
              href="#download"
              className="inline-flex items-center justify-center rounded-full bg-white px-5 py-2 text-sm font-medium text-black hover:bg-neutral-200 transition-colors"
            >
              Get ApexShot
            </Link>
          </div>

          {/* Mobile Menu Button */}
          <button
            onClick={() => setIsOpen(!isOpen)}
            className="md:hidden p-2 text-white"
            aria-label="Toggle menu"
          >
            {isOpen ? <X size={24} /> : <Menu size={24} />}
          </button>
        </div>

        {/* Mobile Navigation */}
        <AnimatePresence>
          {isOpen && (
            <motion.div
              initial={{ opacity: 0, y: -10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              className="md:hidden mt-2"
            >
              <div className="rounded-2xl bg-neutral-900/95 backdrop-blur-md border border-neutral-800 p-4">
                <div className="flex flex-col gap-2">
                  {navLinks.map((link) => (
                    <Link
                      key={link.href}
                      href={link.href}
                      onClick={() => setIsOpen(false)}
                      className="px-4 py-2 text-sm text-neutral-300 hover:text-white hover:bg-neutral-800 rounded-lg transition-colors"
                    >
                      {link.label}
                    </Link>
                  ))}
                  <div className="pt-2 border-t border-neutral-800 mt-2">
                    <Link
                      href="#download"
                      onClick={() => setIsOpen(false)}
                      className="flex items-center justify-center rounded-full bg-white px-5 py-2.5 text-sm font-medium text-black hover:bg-neutral-200 transition-colors"
                    >
                      Get ApexShot
                    </Link>
                  </div>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </nav>
    </header>
  );
}
