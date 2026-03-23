export function actionBadgeClass(action: string): string {
  switch (action) {
    case 'create':
      return 'bg-green-100 text-green-700'
    case 'delete':
      return 'bg-red-100 text-red-700'
    case 'update':
      return 'bg-blue-100 text-blue-700'
    case 'deactivate':
      return 'bg-amber-100 text-amber-700'
    case 'reactivate':
      return 'bg-teal-100 text-teal-700'
    case 'add_member':
    case 'add_inheritance':
    case 'assign':
      return 'bg-green-100 text-green-700'
    case 'remove_member':
    case 'remove_inheritance':
    case 'unassign':
      return 'bg-red-100 text-red-700'
    default:
      return 'bg-gray-100 text-gray-600'
  }
}
