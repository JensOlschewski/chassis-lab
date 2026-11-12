function conMotion = calculateConMotion(iPoint, jPoint, iBasis, jBasis, conType, varargin)
    % ##################################################################################
       % calculateConMotion computes the translational and rotational conMotion
       % between iPoint and jPoint, as well as iBasis and jBasis, considering the
       % free length (freeLength).
       %
       % Inputs:
       %   iPoint, jPoint - Points in i and j body (3x1 vectors)
       %   iBasis, jBasis - (Optional) Orientation rotation matrices (3x3 matrices)
       %   conType        - Type of connection ('spring', 'bushing', 'joint')
       %   freeLength     - (optional) User-defined (scalar or 6x1 vector) required 
       %                     for 'spring' and 'bushing conType
       %
       % Output:
       %   conMotion - Translational and rotational conMotion (6x1 vector)
       % ##################################################################################
   
       % Check if freeLength is provided
       if ~isempty(varargin)
           freeLength = varargin{1};
       else
           freeLength = [];
       end
   
       switch conType
           case "spring"
   
               if isempty(freeLength)
                   error('The parameter freeLength must be provided for the connection type "spring".');
               end
               % Calculate translational conMotion for the spring
               conMotion = norm(iPoint - jPoint) - freeLength;
   
           case "bushing"
   
               if isempty(freeLength)
                   error('The parameter freeLength must be provided for the connection type "bushing".');
               end
               
               % Initialize the conMotion vector
               conMotion = zeros(6, 1);
   
               % Calculate translational conMotion for the bushing
               if ~isequal(iPoint, jPoint)
                   conMotion(1:3) = freeLength(1:3) - jBasis' * (iPoint - jPoint);
               end
   
               % Calculate rotational conMotion for the bushing
               if ~isequal(iBasis, jBasis)
                   conMotion(4:6) = freeLength(4:6) - basis2angle(iBasis, jBasis,'projected');
               end
   
           case "joint"
               % Initialize the conMotion vector
               conMotion = zeros(6, 1);
   
               % Calculate translational motion
               if ~isequal(iPoint, jPoint)
                   conMotion(1:3) = - jBasis' * (iPoint - jPoint);
               end
   
               % Calculate projected rotational motion
               if ~isequal(iBasis, jBasis)
                   conMotion(4:6) = basis2angle(iBasis, jBasis,'projected');
               end
   
           otherwise
               error('Invalid connection type. Choose either "spring" or "bushing".');
       end
   
       % Apply tolerance
       conMotion = applyTolerance(conMotion, 1e-10);
   end