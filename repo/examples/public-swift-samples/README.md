Public SWIFT FIN Samples
========================

These fixtures are public sample messages extracted from:

- Midclear, "Instructions Concerning Category 54x SWIFT Messages", Version 3.2.
  Source URL: https://midclear.com.lb/Files/RulesAndProcedures/LocalMarkets/SWIFT%20Messages%2054x%20Ver3.2%20Local%20Markets.pdf
- Clearstream, "Xact via Swift: New MT380 Foreign Exchange Order", D25006.
  Source URL: https://www.clearstream.com/clearstream-en/securities-services/connectivity-1-/d25006-4297508
- Citi, "SR2018 Change Summary", ISO 15022.
  Source URL: https://www.citigroup.com/mss/sa/dcc/swift/iso_15022/docs/2018/SR2018-Change-Summary-20180920.pdf
- TIBCO ActiveMatrix BusinessWorks Plug-in for SWIFT documentation, MT535 render example.
  Source URL: https://docs.tibco.com/pub/bwpluginswift/6.8.0/doc/html/examples/running-the-project4_Render.htm
- HKEX, "CCASS/3 Message Specification for Participant Supplied System (PSS)", Version 4.7.
  Source URL: https://www.hkex.com.hk/-/media/HKEX-Market/Services/Clearing/Securities/Infrastructure/CCASS-3-Paricipant-Gateway/PG-Messaging-Specifications/1%2C-d%2C-CCASS-3-Messaging-Specifications-for-Participant-Supplied-System-%28PSS%29-Part-1/CCASS3MsgSpecPart1.PDF
- Clearstream, "Vestima+ SWIFT ISO 15022 User Guide for Order Handling Agents", March 2009.
  Source URL: https://www.clearstream.com/resource/blob/1319938/157d5ff631742763cf24fd60ab9d51ec/swift-iso-15022-oha-withdrawn-data.pdf
- Prowide Core / WIFE SourceForge discussion, "Parsing MT564", 2007.
  Source URL: https://sourceforge.net/p/wife/discussion/544817/thread/1d6d5238/
- Clearstream, "Connectivity Handbook Part 2", December 2025.
  Source URL: https://clearstream.com/resource/blob/1312490/33538f0a6f8d43d3d54a925a73309fd6/cbf-connectivity-handbook-part-2-en-data.pdf
- Eurex Clearing, "CCP Release 12.0 Member File Based & SWIFT Interface", 2016.
  Source URL: https://www.eurex.com/resource/blob/301466/08fc443ad22ba8e5026d8b521756faf9/data/ccp-120-memberfile-based-%26-swift-interface-ccp-120.pdf
- Russian National SWIFT Association, "SWIFT-RUS", Moscow, December 2010.
  Source URL: https://www.rosswift.ru/doc/SWIFT-RUS_12102011%28eng%29.pdf
- Russian National SWIFT Association, "SWIFT-RUS", Moscow, February 2014.
  Source URL: https://www.rosswift.ru/doc/SWIFT-RUS9%28eng%29_02_2014.pdf
- KDD Central Securities Clearing Corporation, "SI DVP and FOP Settlement Market Practice".
  Source URL: https://www.kdd.si/_files/1176/SI%20FOP%20and%20DVP%20settlement%20-%20Market%20Practice.pdf

The PDF publishes local-market examples for MT540, MT541, MT542, and MT543,
including cancellation variants. The source examples are block 4 field content;
these files add a `{4: ... -}` envelope so they can be parsed by swiftpipe.

Several other sources publish partial or block-4-only examples. For consistency,
these fixtures keep the published block 4 fields and add a FIN text-block
envelope when the source omits one.

Some PDF-extracted examples include spacing artifacts such as `:16R: GENL`.
Those have been normalized to valid FIN field values, for example `:16R:GENL`.
Obvious page-number extraction artifacts embedded in tags, for example
`:16R:CACONF156`, have likewise been normalized.

One source quirk is preserved intentionally: the Midclear MT540 cancellation
example uses `:23G:NEWM` even though the same document describes cancellation
messages as `:23G:CANC`.
