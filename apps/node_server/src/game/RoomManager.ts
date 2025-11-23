import { Room } from "./Room";

// Characters to use for room codes (excluding confusing chars like 0, O, I, L)
const ROOM_CODE_CHARS = "ABCDEFGHJKMNPQRSTUVWXYZ";
const ROOM_CODE_LENGTH = 6;

class RoomManager {
  private rooms: Map<string, Room>;

  constructor() {
    this.rooms = new Map();
  }

  private generateRoomCode(): string {
    let code: string;
    do {
      code = "";
      for (let i = 0; i < ROOM_CODE_LENGTH; i++) {
        code += ROOM_CODE_CHARS.charAt(
          Math.floor(Math.random() * ROOM_CODE_CHARS.length)
        );
      }
    } while (this.rooms.has(code));
    return code;
  }

  private generateToken(): string {
    return crypto.randomUUID();
  }

  createRoom(): { room: Room; code: string; hostToken: string } {
    const code = this.generateRoomCode();
    const hostToken = this.generateToken();
    const room = new Room(code, hostToken);
    this.rooms.set(code, room);
    return { room, code, hostToken };
  }

  getRoom(code: string): Room | undefined {
    return this.rooms.get(code.toUpperCase());
  }

  deleteRoom(code: string): boolean {
    const room = this.rooms.get(code);
    if (room) {
      room.cleanup();
      this.rooms.delete(code);
      return true;
    }
    return false;
  }

  generatePlayerToken(): string {
    return this.generateToken();
  }

  getRoomCount(): number {
    return this.rooms.size;
  }
}

// Singleton instance
export const roomManager = new RoomManager();
