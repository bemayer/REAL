import * as anchor from "@coral-xyz/anchor";
import { Program, web3 } from "@coral-xyz/anchor";
import { TokenManager } from "../target/types/token_manager.js";
import { PublicKey } from "@solana/web3.js";
import { TOKEN_2022_PROGRAM_ID, getMint } from "@solana/spl-token";
import { expect } from "chai";

describe("Token Manager Program", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.TokenManager as Program<TokenManager>;

  const [tokenManagerPDA] = PublicKey.findProgramAddressSync(
    [Buffer.from("token-manager"), provider.wallet.publicKey.toBuffer()],
    program.programId,
  );

  describe("Initialization", () => {
    it("should explicitly deploy the TokenManager account and verify deployment", async () => {
      const txSig = await program.methods
        .initializeTokenManager()
        .accounts({
          signer: provider.wallet.publicKey,
        })
        .rpc();
      await provider.connection.confirmTransaction(txSig, "confirmed");

      const tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      expect(tokenManagerAccount.tokens).to.be.an("array").that.is.empty;
    });
  });

  const tokensToCreate = [
    { decimals: 6, isin: "US1234567890" },
    { decimals: 8, isin: "US9876543210" },
    { decimals: 2, isin: "EU1234567890" },
  ];

  describe("Multiple Token Creation", () => {
    tokensToCreate.forEach((tokenData, idx) => {
      it(`should create token share ${idx + 1} with ${tokenData.decimals} decimals and ISIN ${tokenData.isin}`, async () => {
        let tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
        const index = tokenManagerAccount.tokens.length;
        const tokenIndexBuffer = Buffer.alloc(8);
        tokenIndexBuffer.writeBigUInt64LE(BigInt(index), 0);

        const [mintPDA] = PublicKey.findProgramAddressSync(
          [Buffer.from("token"), tokenManagerPDA.toBuffer(), tokenIndexBuffer],
          program.programId,
        );

        const txSig = await program.methods
          .createNewShare(tokenData.decimals, tokenData.isin)
          .accounts({
            signer: provider.wallet.publicKey,
            tokenProgram: TOKEN_2022_PROGRAM_ID,
          })
          .rpc();
        expect(txSig).to.be.a("string").that.is.not.empty;
        await provider.connection.confirmTransaction(txSig, "confirmed");

        const mintInfo = await getMint(
          provider.connection,
          mintPDA,
          "confirmed",
          TOKEN_2022_PROGRAM_ID
        );
        expect(mintInfo.decimals).to.equal(tokenData.decimals);
        expect(mintInfo.supply.toString()).to.equal("0");

        tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
        expect(tokenManagerAccount.tokens[index].mint.toString()).to.equal(mintPDA.toString());
        expect(tokenManagerAccount.tokens[index].isin).to.equal(tokenData.isin);
      });
    });
  });

  describe("Token Manager Data Verification", () => {
    it("should have the correct total number of tokens deployed", async () => {
      const tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      expect(tokenManagerAccount.tokens.length).to.equal(tokensToCreate.length);
    });

    it("should verify that each token's stored mint address and ISIN are correctly derived", async () => {
      const tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      for (let i = 0; i < tokenManagerAccount.tokens.length; i++) {
        const tokenIndexBuffer = Buffer.alloc(8);
        tokenIndexBuffer.writeBigUInt64LE(BigInt(i), 0);
        const [expectedMintPDA] = PublicKey.findProgramAddressSync(
          [Buffer.from("token"), tokenManagerPDA.toBuffer(), tokenIndexBuffer],
          program.programId,
        );
        expect(tokenManagerAccount.tokens[i].mint.toString()).to.equal(expectedMintPDA.toString());
        expect(tokenManagerAccount.tokens[i].isin).to.equal(tokensToCreate[i].isin);
      }
    });
  });

  describe("Whitelist Tests", () => {
    const walletsToTest = [
      web3.Keypair.generate(),
      web3.Keypair.generate(),
      web3.Keypair.generate(),
    ];

    it("should fail when adding a wallet to the whitelist if the token ISIN does not exist", async () => {
      const nonExistentIsin = "DOES_NOT_EXIST";

      try {
        await program.methods
          .addToWhitelist(walletsToTest[0].publicKey, nonExistentIsin)
          .accounts({
            signer: provider.wallet.publicKey,
          })
          .rpc();
        expect.fail("Expected error when adding to a non-existent token");
      } catch (err: any) {
        expect(err.error.errorCode.code).to.equal("TokenNotFound");
      }
    });

    it("should fail when removing a wallet from the whitelist if the token ISIN does not exist", async () => {
      const nonExistentIsin = "DOES_NOT_EXIST";

      try {
        await program.methods
          .removeFromWhitelist(walletsToTest[0].publicKey, nonExistentIsin)
          .accounts({
            signer: provider.wallet.publicKey,
          })
          .rpc();
        expect.fail("Expected error when removing from a non-existent token");
      } catch (err: any) {
        expect(err.error.errorCode.code).to.equal("TokenNotFound");
      }
    });

    it("should fail when removing a wallet that is not in the whitelist", async () => {
      const validIsin = tokensToCreate[0].isin;
      const randomWallet = web3.Keypair.generate();

      try {
        await program.methods
          .removeFromWhitelist(randomWallet.publicKey, validIsin)
          .accounts({
            signer: provider.wallet.publicKey,
          })
          .rpc();
        expect.fail("Expected error when removing a wallet not in the whitelist");
      } catch (err: any) {
        expect(err.error.errorCode.code).to.equal("WalletNotFound");
      }
    });

    //
    // 2. Adding Multiple Wallets for Multiple ISINs
    //

    it("should add multiple wallets to the whitelist for multiple token ISINs", async () => {
      // First, fetch current whitelist count for baseline
      let tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      const initialWhitelistCount = tokenManagerAccount.whitelist.length;

      // Add each wallet to each token
      for (const tokenData of tokensToCreate) {
        for (const wallet of walletsToTest) {
          const txSig = await program.methods
            .addToWhitelist(wallet.publicKey, tokenData.isin)
            .accounts({
              signer: provider.wallet.publicKey,
            })
            .rpc();

          expect(txSig).to.be.a("string").that.is.not.empty;
          await provider.connection.confirmTransaction(txSig, "confirmed");
        }
      }

      // Confirm the total number of whitelist entries has increased correctly
      tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      const finalWhitelistCount = tokenManagerAccount.whitelist.length;

      // We added (numberOfTokens * numberOfWallets) new entries
      const expectedNewEntries = tokensToCreate.length * walletsToTest.length;
      expect(finalWhitelistCount).to.equal(initialWhitelistCount + expectedNewEntries);

      // Verify each wallet→token pair is indeed present
      for (const tokenData of tokensToCreate) {
        // Find the token's mint from the tokens array
        const tokenInfo = tokenManagerAccount.tokens.find(
          (t) => t.isin === tokenData.isin
        );
        expect(tokenInfo).to.not.be.undefined;

        for (const wallet of walletsToTest) {
          const foundEntry = tokenManagerAccount.whitelist.find(
            (auth) =>
              auth.mint.toString() === tokenInfo.mint.toString() &&
              auth.authority.toString() === wallet.publicKey.toString()
          );
          expect(foundEntry).to.not.be.undefined;
        }
      }
    });

    it("should remove multiple wallets from the whitelist for multiple token ISINs", async () => {
      // Fetch current state for baseline
      let tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      const initialWhitelistCount = tokenManagerAccount.whitelist.length;

      // Remove each wallet from each token
      for (const tokenData of tokensToCreate) {
        for (const wallet of walletsToTest) {
          const txSig = await program.methods
            .removeFromWhitelist(wallet.publicKey, tokenData.isin)
            .accounts({
              signer: provider.wallet.publicKey,
            })
            .rpc();

          expect(txSig).to.be.a("string").that.is.not.empty;
          await provider.connection.confirmTransaction(txSig, "confirmed");
        }
      }

      // Confirm the total number of whitelist entries has decreased correctly
      tokenManagerAccount = await program.account.tokenManager.fetch(tokenManagerPDA);
      const finalWhitelistCount = tokenManagerAccount.whitelist.length;

      // We removed (numberOfTokens * numberOfWallets) entries
      const expectedRemovedEntries = tokensToCreate.length * walletsToTest.length;
      expect(finalWhitelistCount).to.equal(initialWhitelistCount - expectedRemovedEntries);

      // Double-check that each wallet→token pair is indeed removed
      for (const tokenData of tokensToCreate) {
        // Find the token's mint from the tokens array
        const tokenInfo = tokenManagerAccount.tokens.find(
          (t) => t.isin === tokenData.isin
        );
        expect(tokenInfo).to.not.be.undefined;

        for (const wallet of walletsToTest) {
          const foundEntry = tokenManagerAccount.whitelist.find(
            (auth) =>
              auth.mint.toString() === tokenInfo.mint.toString() &&
              auth.authority.toString() === wallet.publicKey.toString()
          );
          expect(foundEntry).to.be.undefined;
        }
      }
    });
  });
});
